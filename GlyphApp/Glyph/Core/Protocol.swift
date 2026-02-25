//
//  Protocol.swift
//  Glyph
//
//  Protocol message types for Core <-> UI communication
//

import Foundation

// MARK: - Messages to Core

struct ClientCapabilities: Codable, Sendable, Equatable {
    let patchStreaming: Bool

    enum CodingKeys: String, CodingKey {
        case patchStreaming = "patch_streaming"
    }
}

/// Messages sent from UI to Core (matches Rust UiToCore enum)
enum UiToCore: Codable {
    case hello(clientVersion: String, capabilities: ClientCapabilities?)
    case newView(path: String?)
    case closeView(viewId: UInt64)
    case input(viewId: UInt64, event: InputEvent)
    case applyEdit(
        viewId: UInt64,
        start: Int,
        deletedLen: Int,
        insertedText: String,
        baseRevision: UInt64
    )
    case getContent(viewId: UInt64)
    case chat(message: String, contextFiles: [String])
    case requestCompletion(viewId: UInt64, position: Int)
    case indexFile(path: String)
    case querySymbols(path: String)
    case searchSymbols(query: String)

    private enum CodingKeys: String, CodingKey {
        case Hello, NewView, CloseView, Input, ApplyEdit, GetContent, Chat, RequestCompletion
        case IndexFile, QuerySymbols, SearchSymbols
        case client_version, capabilities, path, view_id, event, message
        case context_files, position, query
        case start, deleted_len, inserted_text, base_revision
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .hello(let version, let capabilities):
            var nested = container.nestedContainer(keyedBy: CodingKeys.self, forKey: .Hello)
            try nested.encode(version, forKey: .client_version)
            try nested.encodeIfPresent(capabilities, forKey: .capabilities)
        case .newView(let path):
            var nested = container.nestedContainer(keyedBy: CodingKeys.self, forKey: .NewView)
            try nested.encodeIfPresent(path, forKey: .path)
        case .closeView(let viewId):
            var nested = container.nestedContainer(keyedBy: CodingKeys.self, forKey: .CloseView)
            try nested.encode(viewId, forKey: .view_id)
        case .input(let viewId, let event):
            var nested = container.nestedContainer(keyedBy: CodingKeys.self, forKey: .Input)
            try nested.encode(viewId, forKey: .view_id)
            try nested.encode(event, forKey: .event)
        case .applyEdit(let viewId, let start, let deletedLen, let insertedText, let baseRevision):
            var nested = container.nestedContainer(keyedBy: CodingKeys.self, forKey: .ApplyEdit)
            try nested.encode(viewId, forKey: .view_id)
            try nested.encode(start, forKey: .start)
            try nested.encode(deletedLen, forKey: .deleted_len)
            try nested.encode(insertedText, forKey: .inserted_text)
            try nested.encode(baseRevision, forKey: .base_revision)
        case .getContent(let viewId):
            var nested = container.nestedContainer(keyedBy: CodingKeys.self, forKey: .GetContent)
            try nested.encode(viewId, forKey: .view_id)
        case .chat(let message, let contextFiles):
            var nested = container.nestedContainer(keyedBy: CodingKeys.self, forKey: .Chat)
            try nested.encode(message, forKey: .message)
            try nested.encode(contextFiles, forKey: .context_files)
        case .requestCompletion(let viewId, let position):
            var nested = container.nestedContainer(keyedBy: CodingKeys.self, forKey: .RequestCompletion)
            try nested.encode(viewId, forKey: .view_id)
            try nested.encode(position, forKey: .position)
        case .indexFile(let path):
            var nested = container.nestedContainer(keyedBy: CodingKeys.self, forKey: .IndexFile)
            try nested.encode(path, forKey: .path)
        case .querySymbols(let path):
            var nested = container.nestedContainer(keyedBy: CodingKeys.self, forKey: .QuerySymbols)
            try nested.encode(path, forKey: .path)
        case .searchSymbols(let query):
            var nested = container.nestedContainer(keyedBy: CodingKeys.self, forKey: .SearchSymbols)
            try nested.encode(query, forKey: .query)
        }
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        if let nested = try? container.nestedContainer(keyedBy: CodingKeys.self, forKey: .Hello) {
            let version = try nested.decode(String.self, forKey: .client_version)
            let capabilities = try nested.decodeIfPresent(ClientCapabilities.self, forKey: .capabilities)
            self = .hello(clientVersion: version, capabilities: capabilities)
        } else if let nested = try? container.nestedContainer(keyedBy: CodingKeys.self, forKey: .NewView) {
            let path = try nested.decodeIfPresent(String.self, forKey: .path)
            self = .newView(path: path)
        } else if let nested = try? container.nestedContainer(keyedBy: CodingKeys.self, forKey: .CloseView) {
            let viewId = try nested.decode(UInt64.self, forKey: .view_id)
            self = .closeView(viewId: viewId)
        } else if let nested = try? container.nestedContainer(keyedBy: CodingKeys.self, forKey: .Input) {
            let viewId = try nested.decode(UInt64.self, forKey: .view_id)
            let event = try nested.decode(InputEvent.self, forKey: .event)
            self = .input(viewId: viewId, event: event)
        } else if let nested = try? container.nestedContainer(keyedBy: CodingKeys.self, forKey: .ApplyEdit) {
            let viewId = try nested.decode(UInt64.self, forKey: .view_id)
            let start = try nested.decode(Int.self, forKey: .start)
            let deletedLen = try nested.decode(Int.self, forKey: .deleted_len)
            let insertedText = try nested.decode(String.self, forKey: .inserted_text)
            let baseRevision = try nested.decode(UInt64.self, forKey: .base_revision)
            self = .applyEdit(
                viewId: viewId,
                start: start,
                deletedLen: deletedLen,
                insertedText: insertedText,
                baseRevision: baseRevision
            )
        } else if let nested = try? container.nestedContainer(keyedBy: CodingKeys.self, forKey: .GetContent) {
            let viewId = try nested.decode(UInt64.self, forKey: .view_id)
            self = .getContent(viewId: viewId)
        } else if let nested = try? container.nestedContainer(keyedBy: CodingKeys.self, forKey: .Chat) {
            let message = try nested.decode(String.self, forKey: .message)
            let contextFiles = try nested.decode([String].self, forKey: .context_files)
            self = .chat(message: message, contextFiles: contextFiles)
        } else if let nested = try? container.nestedContainer(keyedBy: CodingKeys.self, forKey: .RequestCompletion) {
            let viewId = try nested.decode(UInt64.self, forKey: .view_id)
            let position = try nested.decode(Int.self, forKey: .position)
            self = .requestCompletion(viewId: viewId, position: position)
        } else if let nested = try? container.nestedContainer(keyedBy: CodingKeys.self, forKey: .IndexFile) {
            let path = try nested.decode(String.self, forKey: .path)
            self = .indexFile(path: path)
        } else if let nested = try? container.nestedContainer(keyedBy: CodingKeys.self, forKey: .QuerySymbols) {
            let path = try nested.decode(String.self, forKey: .path)
            self = .querySymbols(path: path)
        } else if let nested = try? container.nestedContainer(keyedBy: CodingKeys.self, forKey: .SearchSymbols) {
            let query = try nested.decode(String.self, forKey: .query)
            self = .searchSymbols(query: query)
        } else {
            throw DecodingError.dataCorrupted(.init(codingPath: [], debugDescription: "Unknown message type"))
        }
    }
}

/// Input events from user
enum InputEvent: Codable {
    case insert(text: String)
    case backspace
    case delete
    case move(Movement)
    case select(start: Int, end: Int)
    case undo
    case redo
    case save
    case find(query: String)

    private enum CodingKeys: String, CodingKey {
        case Insert, Backspace, Delete, Move, Select, Undo, Redo, Save, Find
        case text, start, end, query
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .insert(let text):
            var nested = container.nestedContainer(keyedBy: CodingKeys.self, forKey: .Insert)
            try nested.encode(text, forKey: .text)
        case .backspace:
            try container.encodeNil(forKey: .Backspace)
        case .delete:
            try container.encodeNil(forKey: .Delete)
        case .move(let movement):
            try container.encode(movement, forKey: .Move)
        case .select(let start, let end):
            var nested = container.nestedContainer(keyedBy: CodingKeys.self, forKey: .Select)
            try nested.encode(start, forKey: .start)
            try nested.encode(end, forKey: .end)
        case .undo:
            try container.encodeNil(forKey: .Undo)
        case .redo:
            try container.encodeNil(forKey: .Redo)
        case .save:
            try container.encodeNil(forKey: .Save)
        case .find(let query):
            var nested = container.nestedContainer(keyedBy: CodingKeys.self, forKey: .Find)
            try nested.encode(query, forKey: .query)
        }
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        if let nested = try? container.nestedContainer(keyedBy: CodingKeys.self, forKey: .Insert) {
            let text = try nested.decode(String.self, forKey: .text)
            self = .insert(text: text)
        } else if container.contains(.Backspace) {
            self = .backspace
        } else if container.contains(.Delete) {
            self = .delete
        } else if let movement = try? container.decode(Movement.self, forKey: .Move) {
            self = .move(movement)
        } else if let nested = try? container.nestedContainer(keyedBy: CodingKeys.self, forKey: .Select) {
            let start = try nested.decode(Int.self, forKey: .start)
            let end = try nested.decode(Int.self, forKey: .end)
            self = .select(start: start, end: end)
        } else if container.contains(.Undo) {
            self = .undo
        } else if container.contains(.Redo) {
            self = .redo
        } else if container.contains(.Save) {
            self = .save
        } else if let nested = try? container.nestedContainer(keyedBy: CodingKeys.self, forKey: .Find) {
            let query = try nested.decode(String.self, forKey: .query)
            self = .find(query: query)
        } else {
            throw DecodingError.dataCorrupted(.init(codingPath: [], debugDescription: "Unknown event type"))
        }
    }
}

enum Movement: String, Codable, Sendable {
    case left = "Left"
    case right = "Right"
    case up = "Up"
    case down = "Down"
    case lineStart = "LineStart"
    case lineEnd = "LineEnd"
    case documentStart = "DocumentStart"
    case documentEnd = "DocumentEnd"
    case wordLeft = "WordLeft"
    case wordRight = "WordRight"
}

// MARK: - Messages from Core

/// A highlighted region in the text
struct HighlightSpan: Codable, Sendable, Equatable {
    let start: Int
    let end: Int
    let highlight: String
}

/// Symbol information from Core
struct SymbolInfo: Codable, Sendable, Identifiable {
    let name: String
    let kind: String
    let file: String
    let line: Int
    let signature: String?

    var id: String { "\(file):\(line):\(name)" }
}

enum CoreErrorCode: String, Codable, Sendable {
    case unknown = "UNKNOWN"
    case invalidRequest = "INVALID_REQUEST"
    case viewNotFound = "VIEW_NOT_FOUND"
    case bufferNotFound = "BUFFER_NOT_FOUND"
    case staleRevision = "STALE_REVISION"
    case invalidRange = "INVALID_RANGE"
    case invalidUtf8Boundary = "INVALID_UTF8_BOUNDARY"
    case deserializeFailed = "DESERIALIZE_FAILED"
    case frameTooLarge = "FRAME_TOO_LARGE"
    case fileReadFailed = "FILE_READ_FAILED"
    case llmFailure = "LLM_FAILURE"
    case completionTimeout = "COMPLETION_TIMEOUT"
    case indexingFailed = "INDEXING_FAILED"

    init(from decoder: Decoder) throws {
        let raw = try decoder.singleValueContainer().decode(String.self)
        self = CoreErrorCode(rawValue: raw) ?? .unknown
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        try container.encode(rawValue)
    }
}

/// Messages sent from Core to UI (matches Rust CoreToUi enum)
enum CoreToUi: Codable, Sendable {
    case welcome(coreVersion: String)
    case viewCreated(viewId: UInt64, content: String, revision: UInt64)
    case applyPatch(viewId: UInt64, patch: Patch, revision: UInt64)
    case applyHighlights(viewId: UInt64, spans: [HighlightSpan])
    case setContent(viewId: UInt64, content: String, spans: [HighlightSpan], revision: UInt64)
    case chatToken(token: String, done: Bool)
    case ghostText(viewId: UInt64, text: String)
    case fileIndexed(path: String, symbolCount: Int)
    case symbols(symbols: [SymbolInfo])
    case event(CoreEvent)
    case error(message: String, code: CoreErrorCode, retryable: Bool, shouldResync: Bool)

    private enum CodingKeys: String, CodingKey {
        case Welcome, ViewCreated, ApplyPatch, ApplyHighlights, SetContent, ChatToken, GhostText
        case FileIndexed, Symbols, Event, Error
        case core_version, view_id, content, patch, spans, token, done, text, message, symbols
        case path, symbol_count, revision
        case code, retryable, should_resync
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)

        if let nested = try? container.nestedContainer(keyedBy: CodingKeys.self, forKey: .Welcome) {
            let version = try nested.decode(String.self, forKey: .core_version)
            self = .welcome(coreVersion: version)
        } else if let nested = try? container.nestedContainer(keyedBy: CodingKeys.self, forKey: .ViewCreated) {
            let viewId = try nested.decode(UInt64.self, forKey: .view_id)
            let content = try nested.decode(String.self, forKey: .content)
            let revision = try nested.decode(UInt64.self, forKey: .revision)
            self = .viewCreated(viewId: viewId, content: content, revision: revision)
        } else if let nested = try? container.nestedContainer(keyedBy: CodingKeys.self, forKey: .ApplyPatch) {
            let viewId = try nested.decode(UInt64.self, forKey: .view_id)
            let patch = try nested.decode(Patch.self, forKey: .patch)
            let revision = try nested.decode(UInt64.self, forKey: .revision)
            self = .applyPatch(viewId: viewId, patch: patch, revision: revision)
        } else if let nested = try? container.nestedContainer(keyedBy: CodingKeys.self, forKey: .ApplyHighlights) {
            let viewId = try nested.decode(UInt64.self, forKey: .view_id)
            let spans = try nested.decode([HighlightSpan].self, forKey: .spans)
            self = .applyHighlights(viewId: viewId, spans: spans)
        } else if let nested = try? container.nestedContainer(keyedBy: CodingKeys.self, forKey: .SetContent) {
            let viewId = try nested.decode(UInt64.self, forKey: .view_id)
            let content = try nested.decode(String.self, forKey: .content)
            let spans = try nested.decode([HighlightSpan].self, forKey: .spans)
            let revision = try nested.decode(UInt64.self, forKey: .revision)
            self = .setContent(viewId: viewId, content: content, spans: spans, revision: revision)
        } else if let nested = try? container.nestedContainer(keyedBy: CodingKeys.self, forKey: .ChatToken) {
            let token = try nested.decode(String.self, forKey: .token)
            let done = try nested.decode(Bool.self, forKey: .done)
            self = .chatToken(token: token, done: done)
        } else if let nested = try? container.nestedContainer(keyedBy: CodingKeys.self, forKey: .GhostText) {
            let viewId = try nested.decode(UInt64.self, forKey: .view_id)
            let text = try nested.decode(String.self, forKey: .text)
            self = .ghostText(viewId: viewId, text: text)
        } else if let nested = try? container.nestedContainer(keyedBy: CodingKeys.self, forKey: .FileIndexed) {
            let path = try nested.decode(String.self, forKey: .path)
            let symbolCount = try nested.decode(Int.self, forKey: .symbol_count)
            self = .fileIndexed(path: path, symbolCount: symbolCount)
        } else if let nested = try? container.nestedContainer(keyedBy: CodingKeys.self, forKey: .Symbols) {
            let symbols = try nested.decode([SymbolInfo].self, forKey: .symbols)
            self = .symbols(symbols: symbols)
        } else if container.contains(.Event) {
            let event = try container.decode(CoreEvent.self, forKey: .Event)
            self = .event(event)
        } else if let nested = try? container.nestedContainer(keyedBy: CodingKeys.self, forKey: .Error) {
            let message = try nested.decode(String.self, forKey: .message)
            let code = try nested.decodeIfPresent(CoreErrorCode.self, forKey: .code) ?? .unknown
            let retryable = try nested.decodeIfPresent(Bool.self, forKey: .retryable) ?? false
            let shouldResync = try nested.decodeIfPresent(Bool.self, forKey: .should_resync) ?? false
            self = .error(
                message: message,
                code: code,
                retryable: retryable,
                shouldResync: shouldResync
            )
        } else {
            throw DecodingError.dataCorrupted(.init(codingPath: [], debugDescription: "Unknown response type"))
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .welcome(let version):
            var nested = container.nestedContainer(keyedBy: CodingKeys.self, forKey: .Welcome)
            try nested.encode(version, forKey: .core_version)
        case .viewCreated(let viewId, let content, let revision):
            var nested = container.nestedContainer(keyedBy: CodingKeys.self, forKey: .ViewCreated)
            try nested.encode(viewId, forKey: .view_id)
            try nested.encode(content, forKey: .content)
            try nested.encode(revision, forKey: .revision)
        case .applyPatch(let viewId, let patch, let revision):
            var nested = container.nestedContainer(keyedBy: CodingKeys.self, forKey: .ApplyPatch)
            try nested.encode(viewId, forKey: .view_id)
            try nested.encode(patch, forKey: .patch)
            try nested.encode(revision, forKey: .revision)
        case .applyHighlights(let viewId, let spans):
            var nested = container.nestedContainer(keyedBy: CodingKeys.self, forKey: .ApplyHighlights)
            try nested.encode(viewId, forKey: .view_id)
            try nested.encode(spans, forKey: .spans)
        case .setContent(let viewId, let content, let spans, let revision):
            var nested = container.nestedContainer(keyedBy: CodingKeys.self, forKey: .SetContent)
            try nested.encode(viewId, forKey: .view_id)
            try nested.encode(content, forKey: .content)
            try nested.encode(spans, forKey: .spans)
            try nested.encode(revision, forKey: .revision)
        case .chatToken(let token, let done):
            var nested = container.nestedContainer(keyedBy: CodingKeys.self, forKey: .ChatToken)
            try nested.encode(token, forKey: .token)
            try nested.encode(done, forKey: .done)
        case .ghostText(let viewId, let text):
            var nested = container.nestedContainer(keyedBy: CodingKeys.self, forKey: .GhostText)
            try nested.encode(viewId, forKey: .view_id)
            try nested.encode(text, forKey: .text)
        case .fileIndexed(let path, let symbolCount):
            var nested = container.nestedContainer(keyedBy: CodingKeys.self, forKey: .FileIndexed)
            try nested.encode(path, forKey: .path)
            try nested.encode(symbolCount, forKey: .symbol_count)
        case .symbols(let symbols):
            var nested = container.nestedContainer(keyedBy: CodingKeys.self, forKey: .Symbols)
            try nested.encode(symbols, forKey: .symbols)
        case .event(let event):
            try container.encode(event, forKey: .Event)
        case .error(let message, let code, let retryable, let shouldResync):
            var nested = container.nestedContainer(keyedBy: CodingKeys.self, forKey: .Error)
            try nested.encode(message, forKey: .message)
            try nested.encode(code, forKey: .code)
            try nested.encode(retryable, forKey: .retryable)
            try nested.encode(shouldResync, forKey: .should_resync)
        }
    }
}

enum PatchApplyError: Error, LocalizedError {
    case invalidRange(offset: Int, count: Int, available: Int)
    case invalidUTF8

    var errorDescription: String? {
        switch self {
        case .invalidRange(let offset, let count, let available):
            return "Invalid patch range: offset \(offset), count \(count), available \(available)"
        case .invalidUTF8:
            return "Patch produced invalid UTF-8 output"
        }
    }
}

enum PatchOp: Codable, Sendable, Equatable {
    case retain(Int)
    case insert(String)
    case delete(Int)

    private enum CodingKeys: String, CodingKey {
        case Retain, Insert, Delete
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        if let count = try? container.decode(Int.self, forKey: .Retain) {
            self = .retain(count)
        } else if let text = try? container.decode(String.self, forKey: .Insert) {
            self = .insert(text)
        } else if let count = try? container.decode(Int.self, forKey: .Delete) {
            self = .delete(count)
        } else {
            throw DecodingError.dataCorrupted(
                .init(codingPath: decoder.codingPath, debugDescription: "Unknown patch op")
            )
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .retain(let count):
            try container.encode(count, forKey: .Retain)
        case .insert(let text):
            try container.encode(text, forKey: .Insert)
        case .delete(let count):
            try container.encode(count, forKey: .Delete)
        }
    }
}

struct Patch: Codable, Sendable, Equatable {
    let ops: [PatchOp]

    init(ops: [PatchOp] = []) {
        self.ops = ops
    }

    func apply(to text: String) throws -> String {
        let sourceBytes = Array(text.utf8)
        var resultBytes = [UInt8]()
        resultBytes.reserveCapacity(sourceBytes.count)

        var cursor = 0
        for op in ops {
            switch op {
            case .retain(let count):
                guard count >= 0, count <= sourceBytes.count - cursor else {
                    throw PatchApplyError.invalidRange(
                        offset: cursor,
                        count: count,
                        available: sourceBytes.count
                    )
                }
                resultBytes.append(contentsOf: sourceBytes[cursor..<(cursor + count)])
                cursor += count
            case .insert(let inserted):
                resultBytes.append(contentsOf: inserted.utf8)
            case .delete(let count):
                guard count >= 0, count <= sourceBytes.count - cursor else {
                    throw PatchApplyError.invalidRange(
                        offset: cursor,
                        count: count,
                        available: sourceBytes.count
                    )
                }
                cursor += count
            }
        }

        if cursor < sourceBytes.count {
            resultBytes.append(contentsOf: sourceBytes[cursor...])
        }

        guard let patched = String(bytes: resultBytes, encoding: .utf8) else {
            throw PatchApplyError.invalidUTF8
        }
        return patched
    }
}

enum CoreEvent: Codable, Sendable {
    case bufferChanged(viewId: UInt64)
    case cursorMoved(viewId: UInt64, position: Int)
    case error(message: String)
    case suggestionReady

    private enum CodingKeys: String, CodingKey {
        case BufferChanged
        case CursorMoved
        case SuggestionReady
        case Error
        case view_id
        case position
        case message
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        if let nested = try? container.nestedContainer(keyedBy: CodingKeys.self, forKey: .BufferChanged) {
            let viewId = try nested.decode(UInt64.self, forKey: .view_id)
            self = .bufferChanged(viewId: viewId)
        } else if let nested = try? container.nestedContainer(keyedBy: CodingKeys.self, forKey: .CursorMoved) {
            let viewId = try nested.decode(UInt64.self, forKey: .view_id)
            let position = try nested.decode(Int.self, forKey: .position)
            self = .cursorMoved(viewId: viewId, position: position)
        } else if let nested = try? container.nestedContainer(keyedBy: CodingKeys.self, forKey: .Error) {
            let message = try nested.decode(String.self, forKey: .message)
            self = .error(message: message)
        } else if container.contains(.SuggestionReady) {
            self = .suggestionReady
        } else {
            throw DecodingError.dataCorrupted(.init(codingPath: [], debugDescription: "Unknown event"))
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .bufferChanged(let viewId):
            var nested = container.nestedContainer(keyedBy: CodingKeys.self, forKey: .BufferChanged)
            try nested.encode(viewId, forKey: .view_id)
        case .cursorMoved(let viewId, let position):
            var nested = container.nestedContainer(keyedBy: CodingKeys.self, forKey: .CursorMoved)
            try nested.encode(viewId, forKey: .view_id)
            try nested.encode(position, forKey: .position)
        case .error(let message):
            var nested = container.nestedContainer(keyedBy: CodingKeys.self, forKey: .Error)
            try nested.encode(message, forKey: .message)
        case .suggestionReady:
            try container.encodeNil(forKey: .SuggestionReady)
        }
    }
}
