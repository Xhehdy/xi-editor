import Foundation
import XCTest

final class GlyphUITests: XCTestCase {
    private var temporaryDirectories: [URL] = []

    override func setUpWithError() throws {
        continueAfterFailure = false
    }

    override func tearDownWithError() throws {
        for directory in temporaryDirectories {
            try? FileManager.default.removeItem(at: directory)
        }
        temporaryDirectories.removeAll()
    }

    @MainActor
    func testEmptyStateSnapshot() throws {
        let fixture = try makeFixtureWorkspace()
        let app = makeApp(
            rootPath: fixture.root.path,
            sidebarMode: "files",
            useStubChat: true,
            useStubSymbols: true
        )

        app.launch()
        XCTAssertTrue(app.staticTexts["Select a file to edit"].waitForExistence(timeout: 10))
        addSnapshot(named: "Empty State", app: app)
    }

    @MainActor
    func testEditorStateSnapshot() throws {
        let fixture = try makeFixtureWorkspace()
        let app = makeApp(
            rootPath: fixture.root.path,
            sidebarMode: "files",
            bootFile: fixture.sampleFile.path,
            useStubEditor: true,
            useStubChat: true,
            useStubSymbols: true
        )

        app.launch()
        XCTAssertTrue(app.staticTexts["Sample.swift"].waitForExistence(timeout: 10))
        addSnapshot(named: "Editor State", app: app)
    }

    @MainActor
    func testChatStateSnapshot() throws {
        let fixture = try makeFixtureWorkspace()
        let app = makeApp(
            rootPath: fixture.root.path,
            sidebarMode: "chat",
            useStubChat: true,
            useStubSymbols: true
        )

        app.launch()
        XCTAssertTrue(app.staticTexts["Stub assistant ready for UI snapshots."].waitForExistence(timeout: 5))
        addSnapshot(named: "Chat State", app: app)
    }

    @MainActor
    func testChatDetachPopupFlow() throws {
        let fixture = try makeFixtureWorkspace()
        let app = makeApp(
            rootPath: fixture.root.path,
            sidebarMode: "chat",
            useStubChat: true,
            useStubSymbols: true
        )

        app.launch()

        let detachButton = app.buttons["Detach Chat"]
        if !detachButton.waitForExistence(timeout: 2) {
            let sidebarToggle = app.buttons["Show Chat Sidebar"]
            XCTAssertTrue(sidebarToggle.waitForExistence(timeout: 5))
            sidebarToggle.tap()
        }
        XCTAssertTrue(detachButton.waitForExistence(timeout: 5))
        detachButton.tap()

        let dockButton = app.buttons["Dock Chat"]
        XCTAssertTrue(dockButton.waitForExistence(timeout: 5))
        let pinButton = app.buttons["Pin Chat Window"]
        XCTAssertTrue(pinButton.waitForExistence(timeout: 2))
        addSnapshot(named: "Chat Popup Detached", app: app)
        pinButton.tap()

        dockButton.tap()

        XCTAssertFalse(app.buttons["Dock Chat"].waitForExistence(timeout: 1))
        XCTAssertTrue(app.buttons["Detach Chat"].waitForExistence(timeout: 5))
    }

    @MainActor
    func testSymbolSearchSnapshot() throws {
        let fixture = try makeFixtureWorkspace()
        let app = makeApp(
            rootPath: fixture.root.path,
            sidebarMode: "symbols",
            useStubChat: true,
            useStubSymbols: true,
            symbolQuery: "edit"
        )

        app.launch()
        XCTAssertTrue(app.staticTexts["applyEdit"].waitForExistence(timeout: 5))
        addSnapshot(named: "Symbol Search State", app: app)
    }

    private func addSnapshot(named name: String, app: XCUIApplication) {
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = name
        attachment.lifetime = .deleteOnSuccess
        add(attachment)
    }

    private func makeFixtureWorkspace() throws -> (root: URL, sampleFile: URL) {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(
            "glyph-ui-tests-\(UUID().uuidString)",
            isDirectory: true
        )
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        temporaryDirectories.append(root)

        let sampleFile = root.appendingPathComponent("Sample.swift")
        let sampleContents = """
        struct Sample {
            let id: String
        }
        """
        try sampleContents.write(to: sampleFile, atomically: true, encoding: .utf8)
        return (root, sampleFile)
    }

    private func makeApp(
        rootPath: String,
        sidebarMode: String,
        bootFile: String? = nil,
        useStubEditor: Bool = false,
        useStubChat: Bool = false,
        useStubSymbols: Bool = false,
        symbolQuery: String? = nil,
        chatVisible: Bool = false,
        chatPresentation: String? = nil
    ) -> XCUIApplication {
        let app = XCUIApplication()
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        app.launchEnvironment["GLYPH_TEST_ROOT"] = rootPath
        app.launchEnvironment["GLYPH_TEST_SIDEBAR_MODE"] = sidebarMode
        if let bootFile {
            app.launchEnvironment["GLYPH_TEST_BOOT_FILE"] = bootFile
        }
        if let symbolQuery {
            app.launchEnvironment["GLYPH_TEST_SYMBOL_QUERY"] = symbolQuery
        }
        app.launchEnvironment["GLYPH_UI_TEST_STUB_EDITOR"] = useStubEditor ? "1" : "0"
        app.launchEnvironment["GLYPH_UI_TEST_STUB_CHAT"] = useStubChat ? "1" : "0"
        app.launchEnvironment["GLYPH_UI_TEST_STUB_SYMBOLS"] = useStubSymbols ? "1" : "0"
        app.launchEnvironment["GLYPH_TEST_CHAT_VISIBLE"] = chatVisible ? "1" : "0"
        if let chatPresentation {
            app.launchEnvironment["GLYPH_TEST_CHAT_PRESENTATION"] = chatPresentation
        }
        return app
    }
}
