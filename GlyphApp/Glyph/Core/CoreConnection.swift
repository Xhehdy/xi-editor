//
//  CoreConnection.swift
//  Glyph
//
//  Unix socket client for communicating with Glyph Core
//

import Foundation
import Darwin

/// Unix socket client for communicating with Glyph Core.
final class CoreConnection {
    private static let maxFrameSize = 8 * 1024 * 1024

    private var socketFD: Int32 = -1
    private let socketPath: String
    private(set) var isConnected = false

    init(socketPath: String = "/tmp/glyph.sock") {
        self.socketPath = socketPath
    }

    /// Connects to the Glyph core process.
    func connect() throws {
        if socketFD >= 0 {
            disconnect()
        }

        socketFD = socket(AF_UNIX, SOCK_STREAM, 0)
        guard socketFD >= 0 else {
            throw CoreConnectionError.socketCreationFailed(errno: errno)
        }

        var addr = sockaddr_un()
        addr.sun_family = sa_family_t(AF_UNIX)

        let maxPathLength = MemoryLayout.size(ofValue: addr.sun_path)
        guard socketPath.utf8.count < maxPathLength else {
            close(socketFD)
            socketFD = -1
            throw CoreConnectionError.invalidSocketPath(maxBytes: maxPathLength - 1)
        }

        socketPath.withCString { pathCString in
            withUnsafeMutablePointer(to: &addr.sun_path) { sunPathPtr in
                sunPathPtr.withMemoryRebound(to: CChar.self, capacity: maxPathLength) { pathPtr in
                    _ = memset(pathPtr, 0, maxPathLength)
                    _ = strncpy(pathPtr, pathCString, maxPathLength - 1)
                }
            }
        }

        let addrLength = socklen_t(MemoryLayout<sa_family_t>.size + socketPath.utf8.count + 1)
        let result = withUnsafePointer(to: &addr) { ptr in
            ptr.withMemoryRebound(to: sockaddr.self, capacity: 1) { sockaddrPtr in
                Darwin.connect(socketFD, sockaddrPtr, addrLength)
            }
        }

        guard result >= 0 else {
            let err = errno
            close(socketFD)
            socketFD = -1
            throw CoreConnectionError.connectionFailed(errno: err)
        }

        isConnected = true
    }

    /// Disconnects from the core
    func disconnect() {
        if socketFD >= 0 {
            close(socketFD)
            socketFD = -1
        }
        isConnected = false
    }

    /// Sends a message and receives a response
    func sendMessage(_ data: Data) throws -> Data {
        guard socketFD >= 0 else {
            throw CoreConnectionError.notConnected
        }

        guard data.count <= CoreConnection.maxFrameSize else {
            throw CoreConnectionError.frameTooLarge(bytes: data.count, maxBytes: CoreConnection.maxFrameSize)
        }

        var length = UInt32(data.count).bigEndian
        try withUnsafeBytes(of: &length) { ptr in
            guard let baseAddress = ptr.baseAddress else {
                throw CoreConnectionError.encodingError
            }
            try writeAll(baseAddress, count: ptr.count)
        }

        try data.withUnsafeBytes { ptr in
            guard let baseAddress = ptr.baseAddress else {
                return
            }
            try writeAll(baseAddress, count: ptr.count)
        }

        var responseLengthBytes: UInt32 = 0
        try withUnsafeMutableBytes(of: &responseLengthBytes) { ptr in
            guard let baseAddress = ptr.baseAddress else {
                throw CoreConnectionError.invalidResponse
            }
            try readExact(baseAddress, count: ptr.count)
        }

        let responseLength = Int(UInt32(bigEndian: responseLengthBytes))
        guard responseLength <= CoreConnection.maxFrameSize else {
            throw CoreConnectionError.frameTooLarge(bytes: responseLength, maxBytes: CoreConnection.maxFrameSize)
        }

        if responseLength == 0 {
            return Data()
        }

        var responseData = Data(count: responseLength)
        try responseData.withUnsafeMutableBytes { ptr in
            guard let baseAddress = ptr.baseAddress else {
                throw CoreConnectionError.invalidResponse
            }
            try readExact(baseAddress, count: responseLength)
        }

        return responseData
    }

    private func writeAll(_ bytes: UnsafeRawPointer, count: Int) throws {
        var totalWritten = 0
        while totalWritten < count {
            let written = Darwin.write(socketFD, bytes.advanced(by: totalWritten), count - totalWritten)
            if written > 0 {
                totalWritten += written
                continue
            }
            if written == -1 && errno == EINTR {
                continue
            }
            throw CoreConnectionError.writeFailed(errno: errno)
        }
    }

    private func readExact(_ bytes: UnsafeMutableRawPointer, count: Int) throws {
        var totalRead = 0
        while totalRead < count {
            let readCount = Darwin.read(socketFD, bytes.advanced(by: totalRead), count - totalRead)
            if readCount > 0 {
                totalRead += readCount
                continue
            }
            if readCount == -1 && errno == EINTR {
                continue
            }
            if readCount == 0 {
                throw CoreConnectionError.invalidResponse
            }
            throw CoreConnectionError.readFailed(errno: errno)
        }
    }

    deinit {
        disconnect()
    }
}

enum CoreConnectionError: Error, LocalizedError {
    case socketCreationFailed(errno: Int32)
    case invalidSocketPath(maxBytes: Int)
    case connectionFailed(errno: Int32)
    case notConnected
    case writeFailed(errno: Int32)
    case readFailed(errno: Int32)
    case invalidResponse
    case frameTooLarge(bytes: Int, maxBytes: Int)
    case encodingError
    case decodingError

    var errorDescription: String? {
        switch self {
        case .socketCreationFailed(let errorCode):
            return "Failed to create socket: \(String(cString: strerror(errorCode)))"
        case .invalidSocketPath(let maxBytes):
            return "Socket path is too long (max \(maxBytes) bytes)"
        case .connectionFailed(let errorCode):
            return "Connection failed: \(String(cString: strerror(errorCode)))"
        case .notConnected:
            return "Not connected to core"
        case .writeFailed(let errorCode):
            return "Failed to write to socket: \(String(cString: strerror(errorCode)))"
        case .readFailed(let errorCode):
            return "Failed to read from socket: \(String(cString: strerror(errorCode)))"
        case .invalidResponse:
            return "Invalid response from core"
        case .frameTooLarge(let bytes, let maxBytes):
            return "Frame size \(bytes) exceeds max size \(maxBytes)"
        case .encodingError:
            return "Failed to encode message"
        case .decodingError:
            return "Failed to decode response"
        }
    }
}
