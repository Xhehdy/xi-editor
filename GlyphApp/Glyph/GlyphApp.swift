//
//  GlyphApp.swift
//  Glyph
//
//  Main app entry point
//

import SwiftUI

@main
struct GlyphApp: App {
    var body: some Scene {
        WindowGroup {
            ContentView()
        }
        .windowStyle(.hiddenTitleBar)
        .windowToolbarStyle(.unifiedCompact)
        .commands {
            CommandGroup(after: .newItem) {
                Button("New Window") {
                    // Open new window
                }
                .keyboardShortcut("n", modifiers: [.command, .shift])
            }

            CommandGroup(replacing: .undoRedo) {
                Button("Undo") {
                    NSApp.sendAction(Selector(("undo:")), to: nil, from: nil)
                }
                .keyboardShortcut("z", modifiers: .command)

                Button("Redo") {
                    NSApp.sendAction(Selector(("redo:")), to: nil, from: nil)
                }
                .keyboardShortcut("z", modifiers: [.command, .shift])
            }
        }
    }
}
