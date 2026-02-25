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
    private let titlebarLift: CGFloat = 42
    private let contentTopInset: CGFloat = 8

    // UI State
    @State private var selectedPath: String?
    @State private var openFiles: [String] = []
    @State private var activeFile: String?
    @State private var sidebarMode: SidebarMode = .files
    @AppStorage("glyph.sidebar.mode") private var sidebarModeRawValue = SidebarMode.files.rawValue
    @State private var didApplyLaunchConfiguration = false
    @State private var isChatSidebarVisible = false
    @State private var isChatPopupVisible = false
    @State private var chatPopupWindow: NSWindow?
    @State private var chatPopupCloseObserver: NSObjectProtocol?

    enum SidebarMode: String, CaseIterable, Hashable {
        case files, symbols

        var title: String {
            switch self {
            case .files: return "Files"
            case .symbols: return "Symbols"
            }
        }

        var iconName: String {
            switch self {
            case .files: return "folder"
            case .symbols: return "list.bullet"
            }
        }

        var subtitle: String {
            switch self {
            case .files: return "Browse project files"
            case .symbols: return "Search indexed symbols"
            }
        }
    }

    private var sidebarRootPath: String {
        if let rootFromEnvironment = ProcessInfo.processInfo.environment["GLYPH_TEST_ROOT"],
           !rootFromEnvironment.isEmpty {
            return rootFromEnvironment
        }

        let cwd = FileManager.default.currentDirectoryPath
        if cwd == "/" {
            return FileManager.default.homeDirectoryForCurrentUser.path
        }
        return cwd
    }

    var body: some View {
        NavigationSplitView {
            VStack(spacing: 0) {
                HStack(spacing: 0) {
                    ForEach(SidebarMode.allCases, id: \.self) { mode in
                        Button {
                            sidebarMode = mode
                        } label: {
                            VStack(spacing: 6) {
                                Label(mode.title, systemImage: mode.iconName)
                                    .font(.system(size: 12, weight: sidebarMode == mode ? .semibold : .regular))
                                    .foregroundColor(sidebarMode == mode ? .primary : .secondary)
                                    .frame(maxWidth: .infinity)
                                Rectangle()
                                    .fill(sidebarMode == mode ? Color.accentColor : Color.clear)
                                    .frame(height: 1)
                            }
                            .padding(.top, 4)
                            .padding(.bottom, 4)
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(.horizontal, 8)
                .accessibilityIdentifier("sidebar.mode")
                .overlay(
                    Rectangle()
                        .frame(height: 1)
                        .foregroundColor(Color(NSColor.separatorColor)),
                    alignment: .bottom
                )

                HStack {
                    Text(sidebarMode.subtitle)
                        .font(.caption)
                        .foregroundColor(.secondary)
                        .lineLimit(1)
                    Spacer()
                }
                .padding(.horizontal, 10)
                .padding(.bottom, 4)

                switch sidebarMode {
                case .files:
                    FileTreeView(
                        rootPath: sidebarRootPath,
                        selectedPath: $selectedPath
                    )
                case .symbols:
                    SymbolBrowserView(client: client)
                }
            }
            .background(Color(NSColor.controlBackgroundColor))
            .navigationSplitViewColumnWidth(min: 200, ideal: 250, max: 400)
        } detail: {
            ZStack(alignment: .topTrailing) {
                HStack(spacing: 0) {
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
                    .frame(maxWidth: .infinity, maxHeight: .infinity)

                    if isChatSidebarVisible {
                        Divider()
                        chatSidebar
                    }
                }
                .background(Color(NSColor.windowBackgroundColor))

                Button {
                    toggleChatSidebar()
                } label: {
                    Image(systemName: "sidebar.right")
                }
                .buttonStyle(.plain)
                .help(isChatSidebarVisible ? "Hide chat sidebar" : "Show chat sidebar")
                .font(.system(size: 14, weight: .medium))
                .foregroundColor(.secondary)
                .padding(.top, 6)
                .padding(.trailing, 12)
                .accessibilityLabel(isChatSidebarVisible ? "Hide Chat Sidebar" : "Show Chat Sidebar")
                .accessibilityIdentifier("chat.sidebar.toggle")

            }
        }
        .padding(.top, contentTopInset - titlebarLift)
        .toolbar(removing: .sidebarToggle)
        .toolbarVisibility(.hidden, for: .windowToolbar)
        .background(MainWindowConfigurator())
        .ignoresSafeArea(.container, edges: .top)
        .onAppear {
            sidebarMode = SidebarMode(rawValue: sidebarModeRawValue) ?? .files
            applyLaunchConfigurationIfNeeded()
        }
        .onChange(of: sidebarMode) { _, newMode in
            sidebarModeRawValue = newMode.rawValue
        }
        .onChange(of: selectedPath) { _, newPath in
            if let path = newPath {
                openFile(path)
            }
        }
        .onDisappear {
            closeChatPopupWindow()
        }
        .accessibilityIdentifier("content.view")
    }

    private var projectName: String {
        URL(fileURLWithPath: sidebarRootPath).lastPathComponent
    }

    var emptyState: some View {
        VStack(spacing: 10) {
            Image(systemName: "doc.text.magnifyingglass")
                .font(.system(size: 42, weight: .light))
                .foregroundColor(.secondary)
            Text("Select a file to edit")
                .font(.headline)
            Text("Workspace: \(projectName)")
                .font(.caption)
                .foregroundColor(.secondary)
        }
        .padding(24)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(
            LinearGradient(
                colors: [
                    Color(NSColor.windowBackgroundColor),
                    Color(NSColor.controlBackgroundColor)
                ],
                startPoint: .top,
                endPoint: .bottom
            )
        )
        .accessibilityIdentifier("empty.state")
    }

    private func openFile(_ path: String) {
        if !openFiles.contains(path) {
            openFiles.append(path)
        }
        activeFile = path
    }

    private var chatContextFiles: [String] {
        var files: [String] = []
        if let activeFile, !activeFile.isEmpty {
            files.append(activeFile)
        }
        for path in openFiles where path != activeFile {
            files.append(path)
            if files.count >= 12 {
                break
            }
        }
        return files
    }

    private var chatSidebar: some View {
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                Label("Conversation", systemImage: "bubble.left.and.bubble.right")
                    .font(.caption)
                    .foregroundColor(.secondary)
                Spacer()
                Button {
                    detachChatSidebarToPopup()
                } label: {
                    Image(systemName: "rectangle.3.group.bubble")
                }
                .buttonStyle(.plain)
                .foregroundColor(.secondary)
                .help("Detach chat as popup")
                .accessibilityLabel("Detach Chat")
                .accessibilityIdentifier("chat.sidebar.detach")
                Button {
                    isChatSidebarVisible = false
                } label: {
                    Image(systemName: "xmark")
                }
                .buttonStyle(.plain)
                .foregroundColor(.secondary)
                .help("Close chat sidebar")
                .accessibilityLabel("Close Chat Sidebar")
                .accessibilityIdentifier("chat.sidebar.close")
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .overlay(
                Rectangle()
                    .frame(height: 1)
                    .foregroundColor(Color(NSColor.separatorColor)),
                alignment: .bottom
            )

            ChatView(client: client, showsInlineHeader: false, contextFiles: chatContextFiles)
        }
        .frame(width: 360)
        .background(Color(NSColor.controlBackgroundColor))
        .accessibilityIdentifier("chat.sidebar")
    }

    private func toggleChatSidebar() {
        if isChatPopupVisible {
            dockChatPopupToSidebar()
            return
        }
        isChatSidebarVisible.toggle()
    }

    private func detachChatSidebarToPopup() {
        isChatSidebarVisible = false
        isChatPopupVisible = true
        showChatPopupWindow()
    }

    private func dockChatPopupToSidebar() {
        isChatSidebarVisible = true
        closeChatPopupWindow()
    }

    private func showChatPopupWindow() {
        #if os(macOS)
        if let popupWindow = chatPopupWindow {
            popupWindow.makeKeyAndOrderFront(nil)
            NSApp.activate(ignoringOtherApps: true)
            return
        }

        let popupSize = NSSize(width: 500, height: 620)
        let popupWindow = NSWindow(
            contentRect: NSRect(origin: .zero, size: popupSize),
            styleMask: [.titled, .closable, .resizable, .miniaturizable],
            backing: .buffered,
            defer: false
        )
        popupWindow.title = "Conversation"
        popupWindow.minSize = NSSize(width: 420, height: 420)
        popupWindow.setContentSize(popupSize)
        popupWindow.isReleasedWhenClosed = false
        popupWindow.titleVisibility = .hidden
        popupWindow.titlebarAppearsTransparent = true
        popupWindow.toolbarStyle = .unifiedCompact
        popupWindow.isMovableByWindowBackground = true
        popupWindow.collectionBehavior = [.fullScreenAuxiliary, .moveToActiveSpace]
        popupWindow.backgroundColor = NSColor.windowBackgroundColor

        let popupContent = DetachedChatWindowView(
            client: client,
            contextFiles: chatContextFiles,
            onDock: { dockChatPopupToSidebar() },
            onClose: { closeChatPopupWindow() },
            onPinChanged: { isPinned in
                popupWindow.level = isPinned ? .floating : .normal
            }
        )
        popupWindow.contentViewController = NSHostingController(rootView: popupContent)

        if let parentWindow = NSApp.keyWindow ?? NSApp.mainWindow {
            let parentFrame = parentWindow.frame
            let proposedOrigin = NSPoint(
                x: parentFrame.maxX - popupSize.width - 28,
                y: parentFrame.maxY - popupSize.height - 54
            )
            popupWindow.setFrameOrigin(proposedOrigin)
        } else {
            popupWindow.center()
        }

        chatPopupCloseObserver = NotificationCenter.default.addObserver(
            forName: NSWindow.willCloseNotification,
            object: popupWindow,
            queue: .main
        ) { _ in
            handleChatPopupWindowDidClose()
        }

        chatPopupWindow = popupWindow
        popupWindow.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        #endif
    }

    private func closeChatPopupWindow() {
        #if os(macOS)
        guard let popupWindow = chatPopupWindow else {
            isChatPopupVisible = false
            return
        }
        popupWindow.level = .normal
        popupWindow.close()
        #endif
    }

    private func handleChatPopupWindowDidClose() {
        isChatPopupVisible = false
        if let observer = chatPopupCloseObserver {
            NotificationCenter.default.removeObserver(observer)
            chatPopupCloseObserver = nil
        }
        chatPopupWindow = nil
    }

    private func applyLaunchConfigurationIfNeeded() {
        guard !didApplyLaunchConfiguration else { return }
        didApplyLaunchConfiguration = true

        let env = ProcessInfo.processInfo.environment
        if let modeRaw = env["GLYPH_TEST_SIDEBAR_MODE"] {
            if modeRaw == "chat" {
                sidebarMode = .files
                isChatSidebarVisible = true
            } else if let mode = SidebarMode(rawValue: modeRaw) {
                sidebarMode = mode
            }
        }

        if let bootFile = env["GLYPH_TEST_BOOT_FILE"],
           !bootFile.isEmpty,
           FileManager.default.fileExists(atPath: bootFile) {
            openFile(bootFile)
            selectedPath = bootFile
        }

        if env["GLYPH_TEST_CHAT_VISIBLE"] == "1" {
            if env["GLYPH_TEST_CHAT_PRESENTATION"] == "popup" {
                isChatPopupVisible = true
                isChatSidebarVisible = false
                showChatPopupWindow()
            } else {
                isChatSidebarVisible = true
                isChatPopupVisible = false
                closeChatPopupWindow()
            }
        }
    }
}

private struct MainWindowConfigurator: NSViewRepresentable {
    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        DispatchQueue.main.async {
            guard let window = view.window else { return }
            configure(window)
        }
        return view
    }

    func updateNSView(_ nsView: NSView, context: Context) {
        DispatchQueue.main.async {
            guard let window = nsView.window else { return }
            configure(window)
        }
    }

    private func configure(_ window: NSWindow) {
        window.titleVisibility = .hidden
        window.titlebarAppearsTransparent = true
        window.styleMask.insert(.fullSizeContentView)
        window.isMovableByWindowBackground = true
        window.toolbar = nil
    }
}

private struct DetachedChatWindowView: View {
    @ObservedObject var client: GlyphClient
    let contextFiles: [String]
    let onDock: () -> Void
    let onClose: () -> Void
    let onPinChanged: (Bool) -> Void
    @State private var isPinned = false

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                Label("Conversation", systemImage: "bubble.left.and.bubble.right")
                    .font(.subheadline.weight(.semibold))
                Spacer()
                Button {
                    if isPinned {
                        isPinned = false
                        onPinChanged(false)
                    }
                    onDock()
                } label: {
                    Image(systemName: "sidebar.right")
                }
                .buttonStyle(.plain)
                .foregroundColor(.secondary)
                .help("Dock to sidebar")
                .accessibilityLabel("Dock Chat")
                .accessibilityIdentifier("chat.popup.dock")

                Button {
                    isPinned.toggle()
                    onPinChanged(isPinned)
                } label: {
                    Image(systemName: isPinned ? "pin.fill" : "pin")
                }
                .buttonStyle(.plain)
                .foregroundColor(isPinned ? .accentColor : .secondary)
                .help("Always stay on top")
                .accessibilityLabel("Pin Chat Window")
                .accessibilityIdentifier("chat.popup.pin")

                Button {
                    if isPinned {
                        isPinned = false
                        onPinChanged(false)
                    }
                    onClose()
                } label: {
                    Image(systemName: "xmark")
                }
                .buttonStyle(.plain)
                .foregroundColor(.secondary)
                .help("Close")
                .accessibilityLabel("Close Chat Popup")
                .accessibilityIdentifier("chat.popup.close")
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .background(Color(NSColor.controlBackgroundColor))
            .overlay(
                Rectangle()
                    .frame(height: 1)
                    .foregroundColor(Color(NSColor.separatorColor)),
                alignment: .bottom
            )

            ChatView(client: client, showsInlineHeader: false, contextFiles: contextFiles)
                .accessibilityIdentifier("chat.popup")
        }
        .frame(minWidth: 420, idealWidth: 500, minHeight: 420, idealHeight: 620)
        .background(Color(NSColor.windowBackgroundColor))
    }
}
