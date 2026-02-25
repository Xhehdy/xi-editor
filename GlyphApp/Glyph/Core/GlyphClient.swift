//
//  GlyphClient.swift
//  Glyph
//
//  High-level client for communicating with Glyph Core
//

import Foundation
import Combine

/// Observable client for Glyph Core communication.
@MainActor
final class GlyphClient: ObservableObject {
    private let connection: CoreConnection
    private let encoder = JSONEncoder()
    private let decoder = JSONDecoder()
    private let ioQueue = DispatchQueue(label: "com.glyph.core-connection.io")
    private var viewRevisions: [UInt64: UInt64] = [:]

    @Published private(set) var isConnected = false
    @Published private(set) var coreVersion: String?
    @Published private(set) var lastError: String?
    @Published private(set) var connectionStatus: ConnectionStatus = .disconnected

    enum ConnectionStatus: String {
        case disconnected = "Disconnected"
        case connecting = "Connecting..."
        case connected = "Connected"
        case error = "Error"
    }

    init(socketPath: String = "/tmp/glyph.sock") {
        self.connection = CoreConnection(socketPath: socketPath)
    }

    /// Connects to the core and sends hello
    func connect() async {
        connectionStatus = .connecting
        lastError = nil
        viewRevisions.removeAll()

        do {
            try await runIO { [self] in
                try self.connection.connect()
            }

            let response = try await send(
                .hello(
                    clientVersion: "1.0.0",
                    capabilities: ClientCapabilities(patchStreaming: true)
                )
            )
            guard case .welcome(let version) = response else {
                throw CoreConnectionError.invalidResponse
            }

            coreVersion = version
            isConnected = true
            connectionStatus = .connected
        } catch {
            try? await runIO { [self] in
                self.connection.disconnect()
            }
            lastError = error.localizedDescription
            connectionStatus = .error
            isConnected = false
        }
    }

    /// Disconnects from the core
    func disconnect() {
        ioQueue.async { [connection] in
            connection.disconnect()
        }
        isConnected = false
        coreVersion = nil
        connectionStatus = .disconnected
        viewRevisions.removeAll()
    }

    /// Creates a new view, optionally loading a file
    func newView(path: String? = nil) async throws -> (viewId: UInt64, content: String) {
        let response = try await send(.newView(path: path))
        guard case .viewCreated(let viewId, let content, let revision) = response else {
            throw errorFrom(response: response)
        }
        viewRevisions[viewId] = revision
        return (viewId, content)
    }

    func closeView(viewId: UInt64) async throws {
        _ = try await send(.closeView(viewId: viewId))
        viewRevisions.removeValue(forKey: viewId)
    }

    /// Applies a single contiguous text edit at `startByteOffset`.
    func applyEdit(
        viewId: UInt64,
        startByteOffset: Int,
        deletedByteCount: Int,
        insertedText: String,
        baseContent: String
    ) async throws -> String {
        guard deletedByteCount > 0 || !insertedText.isEmpty else { return baseContent }
        guard startByteOffset >= 0, deletedByteCount >= 0 else {
            throw CoreConnectionError.invalidResponse
        }
        let baseRevision = viewRevisions[viewId] ?? 0

        do {
            let response = try await send(
                .applyEdit(
                    viewId: viewId,
                    start: startByteOffset,
                    deletedLen: deletedByteCount,
                    insertedText: insertedText,
                    baseRevision: baseRevision
                )
            )
            return try await applyMutationResponse(
                response,
                viewId: viewId,
                currentContent: baseContent
            )
        } catch {
            // If edit application desynchronizes client/server content, recover by full refresh.
            if let (content, _) = try? await getContent(viewId: viewId) {
                return content
            }
            throw error
        }
    }

    /// Sends undo command
    func undo(viewId: UInt64) async throws {
        _ = try await sendInput(viewId: viewId, event: .undo)
    }

    /// Sends redo command
    func redo(viewId: UInt64) async throws {
        _ = try await sendInput(viewId: viewId, event: .redo)
    }

    /// Gets the current content of a view
    func getContent(viewId: UInt64) async throws -> (String, [HighlightSpan]) {
        let response = try await send(.getContent(viewId: viewId))
        guard case .setContent(_, let content, let spans, let revision) = response else {
            throw errorFrom(response: response)
        }
        viewRevisions[viewId] = revision
        return (content, spans)
    }

    /// Sends a chat message
    func chat(_ message: String, contextFiles: [String] = []) async throws -> String {
        let response = try await send(.chat(message: message, contextFiles: contextFiles))
        guard case .chatToken(let token, _) = response else {
            throw errorFrom(response: response)
        }
        return token
    }

    /// Request inline completion
    func requestCompletion(viewId: UInt64, position: Int) async throws -> String {
        let response = try await send(.requestCompletion(viewId: viewId, position: position))
        guard case .ghostText(_, let text) = response else {
            throw errorFrom(response: response)
        }
        return text
    }

    /// Index a file
    func indexFile(path: String) async throws {
        let response = try await send(.indexFile(path: path))
        guard case .fileIndexed = response else {
            throw errorFrom(response: response)
        }
    }

    /// Query symbols in a file
    func querySymbols(path: String) async throws -> [SymbolInfo] {
        let response = try await send(.querySymbols(path: path))
        guard case .symbols(let symbols) = response else {
            throw errorFrom(response: response)
        }
        return symbols
    }

    /// Search symbols
    func searchSymbols(query: String) async throws -> [SymbolInfo] {
        let response = try await send(.searchSymbols(query: query))
        guard case .symbols(let symbols) = response else {
            throw errorFrom(response: response)
        }
        return symbols
    }

    /// Sends a message and waits for response
    private func send(_ message: UiToCore) async throws -> CoreToUi {
        let data = try encoder.encode(message)
        let responseData = try await runIO { [self] in
            try self.connection.sendMessage(data)
        }

        let response = try decoder.decode(CoreToUi.self, from: responseData)
        if case .error(let msg) = response {
            lastError = msg
        }

        return response
    }

    private func sendInput(viewId: UInt64, event: InputEvent) async throws -> CoreToUi {
        try await send(.input(viewId: viewId, event: event))
    }

    private func applyMutationResponse(
        _ response: CoreToUi,
        viewId: UInt64,
        currentContent: String
    ) async throws -> String {
        switch response {
        case .applyPatch(let responseViewId, let patch, let revision):
            guard responseViewId == viewId else {
                throw CoreConnectionError.invalidResponse
            }
            viewRevisions[responseViewId] = revision
            return try patch.apply(to: currentContent)
        case .event(let event):
            switch event {
            case .bufferChanged(let responseViewId):
                guard responseViewId == viewId else {
                    throw CoreConnectionError.invalidResponse
                }
                // Legacy compatibility path: if core downgrades patches to BufferChanged,
                // fetch authoritative content to avoid client/server drift.
                let refresh = try await send(.getContent(viewId: viewId))
                guard case .setContent(_, let content, _, let revision) = refresh else {
                    throw errorFrom(response: refresh)
                }
                viewRevisions[responseViewId] = revision
                return content
            case .cursorMoved(let responseViewId, _):
                guard responseViewId == viewId else {
                    throw CoreConnectionError.invalidResponse
                }
                return currentContent
            case .error(let message):
                throw NSError(
                    domain: "GlyphClient",
                    code: 2,
                    userInfo: [NSLocalizedDescriptionKey: message]
                )
            case .suggestionReady:
                return currentContent
            }
        case .setContent(let responseViewId, let content, _, let revision):
            guard responseViewId == viewId else {
                throw CoreConnectionError.invalidResponse
            }
            viewRevisions[responseViewId] = revision
            return content
        case .error(let message):
            throw NSError(
                domain: "GlyphClient",
                code: 3,
                userInfo: [NSLocalizedDescriptionKey: message]
            )
        default:
            throw CoreConnectionError.invalidResponse
        }
    }

    private func runIO<T>(_ operation: @escaping () throws -> T) async throws -> T {
        try await withCheckedThrowingContinuation { continuation in
            ioQueue.async {
                do {
                    continuation.resume(returning: try operation())
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    private func errorFrom(response: CoreToUi) -> Error {
        if case .error(let message) = response {
            return NSError(domain: "GlyphClient", code: 1, userInfo: [NSLocalizedDescriptionKey: message])
        }
        return CoreConnectionError.invalidResponse
    }
}

/// Compatibility alias during Cortex -> Glyph migration.
typealias CortexClient = GlyphClient
