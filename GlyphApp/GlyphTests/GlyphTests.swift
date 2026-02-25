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

    @Test func searchGraphRoundtripWithFiltersAndOffset() throws {
        let message = UiToCore.searchGraph(
            query: "handler",
            limit: 25,
            offset: 5,
            kindFilter: "function",
            fileFilter: "src/lib.rs"
        )

        let encoded = try JSONEncoder().encode(message)
        let raw = try #require(JSONSerialization.jsonObject(with: encoded) as? [String: Any])
        let payload = try #require(raw["SearchGraph"] as? [String: Any])
        #expect((payload["query"] as? String) == "handler")
        #expect((payload["limit"] as? Int) == 25)
        #expect((payload["offset"] as? Int) == 5)
        #expect((payload["kind_filter"] as? String) == "function")
        #expect((payload["file_filter"] as? String) == "src/lib.rs")

        let decoded = try JSONDecoder().decode(UiToCore.self, from: encoded)
        guard case .searchGraph(let query, let limit, let offset, let kindFilter, let fileFilter) = decoded else {
            Issue.record("Expected SearchGraph variant")
            return
        }

        #expect(query == "handler")
        #expect(limit == 25)
        #expect(offset == 5)
        #expect(kindFilter == "function")
        #expect(fileFilter == "src/lib.rs")
    }

    @Test func searchGraphDecodesLegacyPayloadWithoutFilters() throws {
        let payload = #"{"SearchGraph":{"query":"handler","limit":25}}"#.data(using: .utf8)!
        let decoded = try JSONDecoder().decode(UiToCore.self, from: payload)

        guard case .searchGraph(let query, let limit, let offset, let kindFilter, let fileFilter) = decoded else {
            Issue.record("Expected SearchGraph variant")
            return
        }

        #expect(query == "handler")
        #expect(limit == 25)
        #expect(offset == nil)
        #expect(kindFilter == nil)
        #expect(fileFilter == nil)
    }

    @Test func queryGraphContextRoundtrip() throws {
        let message = UiToCore.queryGraphContext(
            query: "refactor handler",
            contextFiles: ["src/lib.rs", "src/router.rs"],
            limit: 8
        )

        let encoded = try JSONEncoder().encode(message)
        let raw = try #require(JSONSerialization.jsonObject(with: encoded) as? [String: Any])
        let payload = try #require(raw["QueryGraphContext"] as? [String: Any])
        #expect((payload["query"] as? String) == "refactor handler")
        #expect((payload["limit"] as? Int) == 8)
        let contextFiles = try #require(payload["context_files"] as? [String])
        #expect(contextFiles == ["src/lib.rs", "src/router.rs"])

        let decoded = try JSONDecoder().decode(UiToCore.self, from: encoded)
        guard case .queryGraphContext(let query, let decodedFiles, let limit) = decoded else {
            Issue.record("Expected QueryGraphContext variant")
            return
        }

        #expect(query == "refactor handler")
        #expect(decodedFiles == ["src/lib.rs", "src/router.rs"])
        #expect(limit == 8)
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
        let patched = try patch.apply(to: "Hello Legacy")
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

    @Test func coreErrorDecodesLegacyPayloadWithDefaults() throws {
        let payload = #"{"Error":{"message":"legacy"}}"#.data(using: .utf8)!
        let decoded = try JSONDecoder().decode(CoreToUi.self, from: payload)

        guard case .error(let message, let code, let retryable, let shouldResync) = decoded else {
            Issue.record("Expected Error variant")
            return
        }

        #expect(message == "legacy")
        #expect(code == .unknown)
        #expect(retryable == false)
        #expect(shouldResync == false)
    }

    @Test func coreErrorDecodesMetadata() throws {
        let payload = #"""
        {"Error":{"message":"stale revision","code":"STALE_REVISION","retryable":true,"should_resync":true}}
        """#.data(using: .utf8)!
        let decoded = try JSONDecoder().decode(CoreToUi.self, from: payload)

        guard case .error(let message, let code, let retryable, let shouldResync) = decoded else {
            Issue.record("Expected Error variant")
            return
        }

        #expect(message == "stale revision")
        #expect(code == .staleRevision)
        #expect(retryable == true)
        #expect(shouldResync == true)
    }

    @Test func graphDataPayloadDecodes() throws {
        let payload = #"""
        {"GraphData":{"nodes":[{"id":1,"kind":"function","name":"apply_edit","file":"src/lib.rs"}],"edges":[{"from_id":1,"to_id":2,"kind":"contains"}]}}
        """#.data(using: .utf8)!
        let decoded = try JSONDecoder().decode(CoreToUi.self, from: payload)

        guard case .graphData(let nodes, let edges) = decoded else {
            Issue.record("Expected GraphData variant")
            return
        }

        #expect(nodes.count == 1)
        #expect(nodes.first?.name == "apply_edit")
        #expect(nodes.first?.kind == "function")
        #expect(edges.count == 1)
        #expect(edges.first?.kind == "contains")
    }

    @Test func graphContextPayloadDecodes() throws {
        let payload = #"""
        {"GraphContext":{"summary":"ranked graph context","items":[{"node":{"id":10,"kind":"function","name":"handler","file":"src/lib.rs"},"score":12.5,"incoming":3,"outgoing":5,"reasons":["name_match","context_file"]}]}}
        """#.data(using: .utf8)!
        let decoded = try JSONDecoder().decode(CoreToUi.self, from: payload)

        guard case .graphContext(let summary, let items) = decoded else {
            Issue.record("Expected GraphContext variant")
            return
        }

        #expect(summary == "ranked graph context")
        #expect(items.count == 1)
        #expect(items.first?.node.name == "handler")
        #expect(items.first?.score == 12.5)
        #expect(items.first?.incoming == 3)
        #expect(items.first?.outgoing == 5)
        #expect(items.first?.reasons == ["name_match", "context_file"])
    }

    @Test func coreErrorDecodesGraphQueryFailedCode() throws {
        let payload = #"""
        {"Error":{"message":"graph query failed","code":"GRAPH_QUERY_FAILED","retryable":true,"should_resync":false}}
        """#.data(using: .utf8)!
        let decoded = try JSONDecoder().decode(CoreToUi.self, from: payload)

        guard case .error(let message, let code, let retryable, let shouldResync) = decoded else {
            Issue.record("Expected Error variant")
            return
        }

        #expect(message == "graph query failed")
        #expect(code == .graphQueryFailed)
        #expect(retryable == true)
        #expect(shouldResync == false)
    }

    @Test func indexQueueStatsPayloadDecodes() throws {
        let payload = #"""
        {"IndexQueueStats":{"stats":{"queued":4,"in_progress":1,"completed":12,"failed":2,"retried":3,"dropped":1,"max_pending":256,"debounce_ms":250,"max_retries":2,"last_error":"transient"}}}
        """#.data(using: .utf8)!
        let decoded = try JSONDecoder().decode(CoreToUi.self, from: payload)

        guard case .indexQueueStats(let stats) = decoded else {
            Issue.record("Expected IndexQueueStats variant")
            return
        }

        #expect(stats.queued == 4)
        #expect(stats.inProgress == 1)
        #expect(stats.completed == 12)
        #expect(stats.maxPending == 256)
        #expect(stats.debounceMs == 250)
        #expect(stats.maxRetries == 2)
        #expect(stats.lastError == "transient")
    }

    @Test func coreErrorDecodesIndexQueueFullCode() throws {
        let payload = #"""
        {"Error":{"message":"queue full","code":"INDEX_QUEUE_FULL","retryable":true,"should_resync":false}}
        """#.data(using: .utf8)!
        let decoded = try JSONDecoder().decode(CoreToUi.self, from: payload)

        guard case .error(_, let code, let retryable, let shouldResync) = decoded else {
            Issue.record("Expected Error variant")
            return
        }

        #expect(code == .indexQueueFull)
        #expect(retryable == true)
        #expect(shouldResync == false)
    }

    @Test func tabBarReducerKeepsActiveWhenClosingInactiveTab() throws {
        let state = TabBarState.reducedStateAfterClosing(
            openFiles: ["/tmp/a.swift", "/tmp/b.swift", "/tmp/c.swift"],
            activeFile: "/tmp/b.swift",
            closing: "/tmp/a.swift"
        )

        #expect(state.openFiles == ["/tmp/b.swift", "/tmp/c.swift"])
        #expect(state.activeFile == "/tmp/b.swift")
    }

    @Test func tabBarReducerMovesSelectionToRightNeighbor() throws {
        let state = TabBarState.reducedStateAfterClosing(
            openFiles: ["/tmp/a.swift", "/tmp/b.swift", "/tmp/c.swift"],
            activeFile: "/tmp/b.swift",
            closing: "/tmp/b.swift"
        )

        #expect(state.openFiles == ["/tmp/a.swift", "/tmp/c.swift"])
        #expect(state.activeFile == "/tmp/c.swift")
    }

    @Test func tabBarReducerMovesSelectionToLeftWhenClosingLastTab() throws {
        let state = TabBarState.reducedStateAfterClosing(
            openFiles: ["/tmp/a.swift", "/tmp/b.swift"],
            activeFile: "/tmp/b.swift",
            closing: "/tmp/b.swift"
        )

        #expect(state.openFiles == ["/tmp/a.swift"])
        #expect(state.activeFile == "/tmp/a.swift")
    }

    @Test func tabBarReducerClearsSelectionWhenLastTabCloses() throws {
        let state = TabBarState.reducedStateAfterClosing(
            openFiles: ["/tmp/a.swift"],
            activeFile: "/tmp/a.swift",
            closing: "/tmp/a.swift"
        )

        #expect(state.openFiles.isEmpty)
        #expect(state.activeFile == nil)
    }
}
