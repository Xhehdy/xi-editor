//
//  ContentView.swift
//  Glyph
//
//  Main window of the editor
//

import SwiftUI
import AppKit

enum GlyphUI {
    enum Space {
        static let s4: CGFloat = 4
        static let s6: CGFloat = 6
        static let s8: CGFloat = 8
        static let s10: CGFloat = 10
        static let s12: CGFloat = 12
        static let s16: CGFloat = 16
        static let s20: CGFloat = 20
        static let s24: CGFloat = 24
    }

    enum Radius {
        static let small: CGFloat = 6
        static let medium: CGFloat = 10
        static let large: CGFloat = 16
    }

    enum Layout {
        static let titlebarLift: CGFloat = 42
        static let contentTopInset: CGFloat = 8
        static let sidebarMinWidth: CGFloat = 200
        static let sidebarIdealWidth: CGFloat = 250
        static let sidebarMaxWidth: CGFloat = 400
        static let chatSidebarWidth: CGFloat = 360
        static let chatPopupWidth: CGFloat = 500
        static let chatPopupHeight: CGFloat = 620
        static let chatPopupMinWidth: CGFloat = 420
        static let chatPopupMinHeight: CGFloat = 420
        static let chatPopupFrameAutosaveName = "GlyphChatPopupFrame"
    }
}

struct WorkspaceSessionSnapshot: Codable, Equatable {
    let openFiles: [String]
    let activeFile: String?
}

enum WorkspaceSessionState {
    static func storageKey(rootPath: String) -> String {
        let normalizedRoot = normalizedPath(rootPath) ?? rootPath
        return "glyph.workspace.session.\(stableHash(normalizedRoot))"
    }

    static func normalizedSnapshot(
        openFiles: [String],
        activeFile: String?,
        fileExists: (String) -> Bool
    ) -> WorkspaceSessionSnapshot {
        var seen = Set<String>()
        var normalizedOpenFiles: [String] = []

        for path in openFiles {
            guard let normalized = normalizedPath(path),
                  fileExists(normalized),
                  seen.insert(normalized).inserted else {
                continue
            }
            normalizedOpenFiles.append(normalized)
        }

        var normalizedActive = normalizedPath(activeFile)
        if let normalizedActive,
           fileExists(normalizedActive),
           !seen.contains(normalizedActive) {
            normalizedOpenFiles.append(normalizedActive)
            seen.insert(normalizedActive)
        }

        if normalizedActive == nil || !normalizedOpenFiles.contains(normalizedActive!) {
            normalizedActive = normalizedOpenFiles.last
        }

        return WorkspaceSessionSnapshot(
            openFiles: normalizedOpenFiles,
            activeFile: normalizedActive
        )
    }

    private static func normalizedPath(_ path: String?) -> String? {
        guard let path,
              !path.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            return nil
        }
        return URL(fileURLWithPath: path).standardizedFileURL.path
    }

    private static func stableHash(_ value: String) -> String {
        var hash: UInt64 = 14_695_981_039_346_656_037
        for byte in value.utf8 {
            hash ^= UInt64(byte)
            hash &*= 1_099_511_628_211
        }
        return String(hash, radix: 16)
    }
}

struct ContentView: View {
    @StateObject private var client = GlyphClient()
    @AppStorage("glyph.sidebar.width") private var sidebarWidthRaw = Double(GlyphUI.Layout.sidebarIdealWidth)
    @AppStorage("glyph.chat.sidebar.visible") private var persistedChatSidebarVisible = false
    @AppStorage("glyph.chat.popup.visible") private var persistedChatPopupVisible = false
    @AppStorage("glyph.chat.popup.pinned") private var persistedChatPopupPinned = false

    // UI State
    @State private var selectedPath: String?
    @State private var openFiles: [String] = []
    @State private var activeFile: String?
    @State private var sidebarWidth = GlyphUI.Layout.sidebarIdealWidth
    @State private var sidebarMode: SidebarMode = .files
    @AppStorage("glyph.sidebar.mode") private var sidebarModeRawValue = SidebarMode.files.rawValue
    @State private var didApplyLaunchConfiguration = false
    @State private var isChatSidebarVisible = false
    @State private var isChatPopupVisible = false
    @State private var chatPopupWindow: NSWindow?
    @State private var chatPopupCloseObserver: NSObjectProtocol?
    @State private var unsavedGuardMessage: String?

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
                            .padding(.top, GlyphUI.Space.s4)
                            .padding(.bottom, GlyphUI.Space.s4)
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(.horizontal, GlyphUI.Space.s8)
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
                .padding(.horizontal, GlyphUI.Space.s10)
                .padding(.bottom, GlyphUI.Space.s4)

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
            .background(SidebarWidthMonitor(sidebarWidth: $sidebarWidth))
            .navigationSplitViewColumnWidth(
                min: GlyphUI.Layout.sidebarMinWidth,
                ideal: sidebarWidth,
                max: GlyphUI.Layout.sidebarMaxWidth
            )
        } detail: {
            ZStack(alignment: .topTrailing) {
                HStack(spacing: 0) {
                    VStack(spacing: 0) {
                        if !openFiles.isEmpty {
                            TabBarView(
                                openFiles: $openFiles,
                                activeFile: $activeFile,
                                dirtyPaths: dirtyOpenFiles,
                                onSelectFile: attemptActivateFile,
                                onCloseFile: attemptCloseFile,
                                onCloseAllFiles: attemptCloseAllFiles
                            )

                            if let file = activeFile {
                                ZStack {
                                    ForEach(openFiles, id: \.self) { openFile in
                                        EditorView(
                                            client: client,
                                            filePath: openFile,
                                            isActive: openFile == file
                                        )
                                        .opacity(openFile == file ? 1 : 0)
                                        .allowsHitTesting(openFile == file)
                                        .accessibilityHidden(openFile != file)
                                    }
                                }
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
                .padding(.top, GlyphUI.Space.s6)
                .padding(.trailing, GlyphUI.Space.s12)
                .accessibilityLabel(isChatSidebarVisible ? "Hide Chat Sidebar" : "Show Chat Sidebar")
                .accessibilityIdentifier("chat.sidebar.toggle")

            }
        }
        .padding(.top, GlyphUI.Layout.contentTopInset - GlyphUI.Layout.titlebarLift)
        .toolbar(removing: .sidebarToggle)
        .toolbarVisibility(.hidden, for: .windowToolbar)
        .background(MainWindowConfigurator())
        .ignoresSafeArea(.container, edges: .top)
        .onAppear {
            sidebarWidth = clampedSidebarWidth(CGFloat(sidebarWidthRaw))
            sidebarMode = SidebarMode(rawValue: sidebarModeRawValue) ?? .files
            applyLaunchConfigurationIfNeeded()
        }
        .onChange(of: sidebarMode) { _, newMode in
            sidebarModeRawValue = newMode.rawValue
        }
        .onChange(of: sidebarWidth) { _, newWidth in
            let clamped = clampedSidebarWidth(newWidth)
            if abs(clamped - sidebarWidth) > 0.5 {
                sidebarWidth = clamped
            }
            sidebarWidthRaw = Double(clamped)
        }
        .onChange(of: isChatSidebarVisible) { _, isVisible in
            persistedChatSidebarVisible = isVisible
        }
        .onChange(of: isChatPopupVisible) { _, isVisible in
            persistedChatPopupVisible = isVisible
        }
        .onChange(of: openFiles) { _, _ in
            persistWorkspaceSession()
        }
        .onChange(of: activeFile) { _, _ in
            persistWorkspaceSession()
        }
        .onChange(of: selectedPath) { _, newPath in
            if let path = newPath {
                attemptOpenFile(path)
            }
        }
        .onDisappear {
            closeChatPopupWindow()
        }
        .alert(
            "Unsaved changes",
            isPresented: Binding(
                get: { unsavedGuardMessage != nil },
                set: { if !$0 { unsavedGuardMessage = nil } }
            )
        ) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(unsavedGuardMessage ?? "")
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
        .padding(GlyphUI.Space.s24)
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

    private var dirtyOpenFiles: Set<String> {
        Set(openFiles.filter { client.isDirty(path: $0) })
    }

    private func openFile(_ path: String) {
        if !openFiles.contains(path) {
            openFiles.append(path)
        }
        activeFile = path
    }

    private func attemptOpenFile(_ path: String) {
        guard path != activeFile else { return }
        openFile(path)
        selectedPath = path
    }

    private func attemptActivateFile(_ path: String) {
        guard path != activeFile else { return }
        activeFile = path
        selectedPath = path
    }

    private func attemptCloseFile(_ path: String) {
        if blockIfPathIsDirty(path, reason: "closing this tab") {
            return
        }

        let reducedState = TabBarState.reducedStateAfterClosing(
            openFiles: openFiles,
            activeFile: activeFile,
            closing: path
        )
        openFiles = reducedState.openFiles
        activeFile = reducedState.activeFile
        selectedPath = reducedState.activeFile
    }

    private func attemptCloseAllFiles() {
        if let firstDirtyPath = openFiles.first(where: { client.isDirty(path: $0) }) {
            _ = blockIfPathIsDirty(firstDirtyPath, reason: "closing open tabs")
            return
        }
        openFiles.removeAll()
        activeFile = nil
        selectedPath = nil
    }

    private func blockIfPathIsDirty(_ path: String, reason: String) -> Bool {
        guard client.isDirty(path: path) else {
            return false
        }

        let name = URL(fileURLWithPath: path).lastPathComponent
        unsavedGuardMessage = "Save or reload \(name) before \(reason)."
        activeFile = path
        selectedPath = path
        return true
    }

    private var workspaceSessionKey: String {
        WorkspaceSessionState.storageKey(rootPath: sidebarRootPath)
    }

    private func persistWorkspaceSession() {
        let snapshot = WorkspaceSessionState.normalizedSnapshot(
            openFiles: openFiles,
            activeFile: activeFile,
            fileExists: { FileManager.default.fileExists(atPath: $0) }
        )
        let defaults = UserDefaults.standard
        if snapshot.openFiles.isEmpty {
            defaults.removeObject(forKey: workspaceSessionKey)
            return
        }
        guard let data = try? JSONEncoder().encode(snapshot),
              let json = String(data: data, encoding: .utf8) else {
            return
        }
        defaults.set(json, forKey: workspaceSessionKey)
    }

    private func restoreWorkspaceSession() {
        let defaults = UserDefaults.standard
        guard let json = defaults.string(forKey: workspaceSessionKey),
              let data = json.data(using: .utf8),
              let snapshot = try? JSONDecoder().decode(WorkspaceSessionSnapshot.self, from: data) else {
            return
        }

        let restored = WorkspaceSessionState.normalizedSnapshot(
            openFiles: snapshot.openFiles,
            activeFile: snapshot.activeFile,
            fileExists: { FileManager.default.fileExists(atPath: $0) }
        )
        guard !restored.openFiles.isEmpty else {
            defaults.removeObject(forKey: workspaceSessionKey)
            return
        }

        openFiles = restored.openFiles
        activeFile = restored.activeFile
        selectedPath = restored.activeFile
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
            .padding(.vertical, GlyphUI.Space.s6)
            .overlay(
                Rectangle()
                    .frame(height: 1)
                    .foregroundColor(Color(NSColor.separatorColor)),
                alignment: .bottom
            )

            ChatView(client: client, showsInlineHeader: false, contextFiles: chatContextFiles)
        }
        .frame(width: GlyphUI.Layout.chatSidebarWidth)
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

        let popupSize = NSSize(width: GlyphUI.Layout.chatPopupWidth, height: GlyphUI.Layout.chatPopupHeight)
        let popupWindow = NSWindow(
            contentRect: NSRect(origin: .zero, size: popupSize),
            styleMask: [.titled, .closable, .resizable, .miniaturizable],
            backing: .buffered,
            defer: false
        )
        popupWindow.title = "Conversation"
        popupWindow.minSize = NSSize(width: GlyphUI.Layout.chatPopupMinWidth, height: GlyphUI.Layout.chatPopupMinHeight)
        popupWindow.setContentSize(popupSize)
        popupWindow.isReleasedWhenClosed = false
        popupWindow.titleVisibility = .hidden
        popupWindow.titlebarAppearsTransparent = true
        popupWindow.toolbarStyle = .unifiedCompact
        popupWindow.isMovableByWindowBackground = true
        popupWindow.collectionBehavior = [.fullScreenAuxiliary, .moveToActiveSpace]
        popupWindow.backgroundColor = NSColor.windowBackgroundColor
        let frameAutosaveName = NSWindow.FrameAutosaveName(GlyphUI.Layout.chatPopupFrameAutosaveName)
        let didRestoreFrame = popupWindow.setFrameUsingName(frameAutosaveName)
        _ = popupWindow.setFrameAutosaveName(frameAutosaveName)
        popupWindow.level = persistedChatPopupPinned ? .floating : .normal

        let popupContent = DetachedChatWindowView(
            client: client,
            contextFiles: chatContextFiles,
            initialPinned: persistedChatPopupPinned,
            onDock: { dockChatPopupToSidebar() },
            onClose: { closeChatPopupWindow() },
            onPinChanged: { isPinned in
                persistedChatPopupPinned = isPinned
                popupWindow.level = isPinned ? .floating : .normal
            }
        )
        popupWindow.contentViewController = NSHostingController(rootView: popupContent)

        if !didRestoreFrame, let parentWindow = NSApp.keyWindow ?? NSApp.mainWindow {
            let parentFrame = parentWindow.frame
            let proposedOrigin = NSPoint(
                x: parentFrame.maxX - popupSize.width - 28,
                y: parentFrame.maxY - popupSize.height - 54
            )
            popupWindow.setFrameOrigin(proposedOrigin)
        } else if !didRestoreFrame {
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
        popupWindow.level = persistedChatPopupPinned ? .floating : .normal
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

        isChatSidebarVisible = persistedChatSidebarVisible
        isChatPopupVisible = persistedChatPopupVisible

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
        } else {
            restoreWorkspaceSession()
        }

        if env["GLYPH_TEST_CHAT_VISIBLE"] == "1" {
            if env["GLYPH_TEST_CHAT_PRESENTATION"] == "popup" {
                isChatPopupVisible = true
                isChatSidebarVisible = false
            } else {
                isChatSidebarVisible = true
                isChatPopupVisible = false
            }
        }

        if isChatPopupVisible {
            isChatSidebarVisible = false
            showChatPopupWindow()
        } else {
            closeChatPopupWindow()
        }
    }

    private func clampedSidebarWidth(_ width: CGFloat) -> CGFloat {
        min(max(width, GlyphUI.Layout.sidebarMinWidth), GlyphUI.Layout.sidebarMaxWidth)
    }
}

private struct SidebarWidthMonitor: NSViewRepresentable {
    @Binding var sidebarWidth: CGFloat

    func makeCoordinator() -> Coordinator {
        Coordinator()
    }

    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        context.coordinator.attach(to: view, sidebarWidth: $sidebarWidth)
        return view
    }

    func updateNSView(_ nsView: NSView, context: Context) {
        context.coordinator.attach(to: nsView, sidebarWidth: $sidebarWidth)
    }

    static func dismantleNSView(_ nsView: NSView, coordinator: Coordinator) {
        coordinator.stop()
    }

    final class Coordinator {
        private weak var hostView: NSView?
        private var timer: Timer?
        private var sidebarWidthBinding: Binding<CGFloat>?
        private var lastWidth: CGFloat = .zero

        func attach(to view: NSView, sidebarWidth: Binding<CGFloat>) {
            hostView = view
            sidebarWidthBinding = sidebarWidth
            guard timer == nil else { return }
            timer = Timer.scheduledTimer(withTimeInterval: 0.25, repeats: true) { [weak self] _ in
                self?.poll()
            }
            timer?.tolerance = 0.08
        }

        func stop() {
            timer?.invalidate()
            timer = nil
            sidebarWidthBinding = nil
            hostView = nil
        }

        private func poll() {
            guard let hostView else { return }
            guard let splitView = findSplitView(startingAt: hostView) else { return }
            guard splitView.subviews.count >= 2 else { return }
            let width = splitView.subviews[0].frame.width
            guard width > 0 else { return }
            if abs(width - lastWidth) > 0.5 {
                lastWidth = width
                guard let sidebarWidthBinding else { return }
                if abs(sidebarWidthBinding.wrappedValue - width) > 0.5 {
                    sidebarWidthBinding.wrappedValue = width
                }
            }
        }

        private func findSplitView(startingAt view: NSView) -> NSSplitView? {
            var node: NSView? = view
            while let current = node {
                if let split = current as? NSSplitView {
                    return split
                }
                node = current.superview
            }
            return nil
        }

        deinit {
            stop()
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
    let initialPinned: Bool
    let onDock: () -> Void
    let onClose: () -> Void
    let onPinChanged: (Bool) -> Void
    @State private var isPinned: Bool

    init(
        client: GlyphClient,
        contextFiles: [String],
        initialPinned: Bool,
        onDock: @escaping () -> Void,
        onClose: @escaping () -> Void,
        onPinChanged: @escaping (Bool) -> Void
    ) {
        self.client = client
        self.contextFiles = contextFiles
        self.initialPinned = initialPinned
        self.onDock = onDock
        self.onClose = onClose
        self.onPinChanged = onPinChanged
        _isPinned = State(initialValue: initialPinned)
    }

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
            .padding(.vertical, GlyphUI.Space.s8)
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
        .frame(
            minWidth: GlyphUI.Layout.chatPopupMinWidth,
            idealWidth: GlyphUI.Layout.chatPopupWidth,
            minHeight: GlyphUI.Layout.chatPopupMinHeight,
            idealHeight: GlyphUI.Layout.chatPopupHeight
        )
        .background(Color(NSColor.windowBackgroundColor))
    }
}
