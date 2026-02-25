//
//  GlyphTests.swift
//  GlyphTests
//
//  Created by Winddy on 30/12/2025.
//

import Foundation
import Testing
@testable import Glyph

@MainActor
struct GlyphTests {
    @Test func helloRoundtripWithPatchStreamingCapability() throws {
        let message = UiToCore.hello(
            clientVersion: "1.0.0",
            capabilities: ClientCapabilities(patchStreaming: true)
        )

        let encoded = try JSONEncoder().encode(message)
        let raw = try #require(JSONSerialization.jsonObject(with: encoded) as? [String: Any])
        let hello = try #require(raw["Hello"] as? [String: Any])
        let caps = try #require(hello["capabilities"] as? [String: Any])
        #expect((caps["patch_streaming"] as? Bool) == true)

        let decoded = try JSONDecoder().decode(UiToCore.self, from: encoded)
        guard case .hello(let version, let capabilities) = decoded else {
            Issue.record("Expected Hello variant")
            return
        }

        #expect(version == "1.0.0")
        #expect(capabilities == ClientCapabilities(patchStreaming: true))
    }

    @Test func helloDecodesWithoutCapabilitiesForLegacyPayload() throws {
        let payload = #"{"Hello":{"client_version":"1.0.0"}}"#.data(using: .utf8)!
        let decoded = try JSONDecoder().decode(UiToCore.self, from: payload)

        guard case .hello(let version, let capabilities) = decoded else {
            Issue.record("Expected Hello variant")
            return
        }

        #expect(version == "1.0.0")
        #expect(capabilities == nil)
    }

    @Test func decodeApplyPatchAndApplyToBuffer() throws {
        let payload = #"""
        {"ApplyPatch":{"view_id":42,"patch":{"ops":[{"Retain":6},{"Delete":6},{"Insert":"Glyph"}]},"revision":7}}
        """#.data(using: .utf8)!
        let message: CoreToUi = try JSONDecoder().decode(CoreToUi.self, from: payload)

        let viewId: UInt64
        let patch: Patch
        let revision: UInt64
        switch message {
        case .applyPatch(let decodedViewId, let decodedPatch, let decodedRevision):
            viewId = decodedViewId
            patch = decodedPatch
            revision = decodedRevision
        default:
            Issue.record("Expected ApplyPatch variant")
            return
        }

        #expect(viewId == 42)
        #expect(revision == 7)
        let patched = try patch.apply(to: "Hello Cortex")
        #expect(patched == "Hello Glyph")
    }

    @Test func patchApplyHandlesUnicodeEdits() throws {
        let patch = Patch(ops: [
            .retain(1),
            .delete(4),
            .insert("🚀")
        ])

        let patched = try patch.apply(to: "A🙂B")
        #expect(patched == "A🚀B")
    }

    @Test func patchApplyRejectsOutOfRangeOps() throws {
        let patch = Patch(ops: [.retain(99)])

        do {
            _ = try patch.apply(to: "hi")
            Issue.record("Expected out-of-range patch failure")
        } catch let error as PatchApplyError {
            guard case .invalidRange = error else {
                Issue.record("Expected invalidRange, got \(error)")
                return
            }
        } catch {
            Issue.record("Expected PatchApplyError, got \(error)")
        }
    }
}
