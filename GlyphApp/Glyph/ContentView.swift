//
//  ContentView.swift
//  Glyph
//
//  Main window of the editor
//

import SwiftUI
import AppKit

struct ContentView: View {
    @StateObject private var client = GlyphClient()

    // UI State
    @State private var selectedPath: String?
    @State private var openFiles: [String] = []
    @State private var activeFile: String?
    @State private var inspectorVisible: Bool = false
    @State private var sidebarMode: SidebarMode = .files

    enum SidebarMode {
        case files, chat, symbols
    }

    private var sidebarRootPath: String {
        let cwd = FileManager.default.currentDirectoryPath
        if cwd == "/" {
            return FileManager.default.homeDirectoryForCurrentUser.path
        }
        return cwd
    }

    var body: some View {
        NavigationSplitView {
            VStack(spacing: 0) {
                // Header (Segmented Control for File/Chat/Symbols)
                Picker("Sidebar", selection: $sidebarMode) {
                    Image(systemName: "folder").tag(SidebarMode.files)
                    Image(systemName: "message").tag(SidebarMode.chat)
                    Image(systemName: "list.bullet").tag(SidebarMode.symbols)
                }
                .pickerStyle(.segmented)
                .padding(8)

                switch sidebarMode {
                case .files:
                    FileTreeView(
                        rootPath: sidebarRootPath,
                        selectedPath: $selectedPath
                    )
                case .chat:
                    ChatView(client: client)
                case .symbols:
                    SymbolBrowserView(client: client)
                }
            }
            .background(Color(NSColor.controlBackgroundColor))
            .navigationSplitViewColumnWidth(min: 200, ideal: 250, max: 400)
        } detail: {
            VStack(spacing: 0) {
                if !openFiles.isEmpty {
                    TabBarView(openFiles: $openFiles, activeFile: $activeFile)

                    if let file = activeFile {
                        EditorView(client: client, filePath: file)
                            .id(file) // Force recreate on file change to reset state
                    } else {
                        emptyState
                    }
                } else {
                    emptyState
                }
            }
        }
        .onChange(of: selectedPath) { _, newPath in
            if let path = newPath {
                openFile(path)
            }
        }
    }

    var emptyState: some View {
        VStack {
            Image(systemName: "command.square")
                .font(.system(size: 48))
                .foregroundColor(.secondary)
            Text("Select a file to edit")
                .foregroundColor(.secondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private func openFile(_ path: String) {
        if !openFiles.contains(path) {
            openFiles.append(path)
        }
        activeFile = path
    }
}
