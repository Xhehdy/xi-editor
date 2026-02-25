//
//  GlyphUITestsLaunchTests.swift
//  GlyphUITests
//
//  Created by Winddy on 30/12/2025.
//

import XCTest

final class GlyphUITestsLaunchTests: XCTestCase {

    override class var runsForEachTargetApplicationUIConfiguration: Bool {
        false
    }

    override func setUpWithError() throws {
        continueAfterFailure = false
    }

    @MainActor
    func testLaunch() throws {
        let app = XCUIApplication()
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES"]
        app.launchEnvironment["GLYPH_UI_TEST_STUB_CHAT"] = "1"
        app.launchEnvironment["GLYPH_UI_TEST_STUB_SYMBOLS"] = "1"
        app.launch()

        XCTAssertTrue(app.staticTexts["Select a file to edit"].waitForExistence(timeout: 10))

        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = "Launch Screen"
        attachment.lifetime = .deleteOnSuccess
        add(attachment)
    }
}
