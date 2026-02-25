//
//  EditorView.swift
//  Glyph
//
//  The main text editor view - talks to Glyph Core
//

import SwiftUI
import AppKit

/// Native macOS text editor view backed by Glyph Core.
struct EditorView: View {
    @ObservedObject var client: GlyphClient
    @State private var text = ""
    @State private var spans: [HighlightSpan] = []
    @State private var viewId: UInt64?
    @State private var isLoading = true
    @State private var errorMessage: String?
    @State private var inlineErrorMessage: String?
    @State private var ghostText: String?
    @State private var cursorByteOffset = 0
    @State private var editPipeline: Task<Void, Never>?
    @State private var ghostTask: Task<Void, Never>?
    @State private var latestEditVersion: UInt64 = 0
    @State private var isSyncing = false

    let filePath: String?

    init(client: GlyphClient, filePath: String? = nil) {
        self.client = client
        self.filePath = filePath
    }

    var body: some View {
        ZStack {
            if isLoading {
                ProgressView("Connecting to Core...")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if let error = errorMessage {
                VStack(spacing: 16) {
                    Image(systemName: "exclamationmark.triangle")
                        .font(.system(size: 48))
                        .foregroundColor(.orange)
                    Text(error)
                        .font(.headline)
                    Button("Retry") {
                        Task { await connectAndLoad() }
                    }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                VStack(spacing: 0) {
                    ZStack(alignment: .topLeading) {
                        NativeTextEditor(
                            text: $text,
                            spans: $spans,
                            onTextChange: handleTextChange,
                            onSelectionChange: { cursorByteOffset = $0 }
                        )
                        .font(.system(.body, design: .monospaced))

                        if let ghost = ghostText, !ghost.isEmpty {
                            Text(ghost)
                                .font(.system(.body, design: .monospaced))
                                .foregroundColor(.gray)
                                .padding(.top, 4)
                                .padding(.leading, 8)
                                .allowsHitTesting(false)
                        }
                    }

                    HStack {
                        if let path = filePath {
                            Text(URL(fileURLWithPath: path).lastPathComponent)
                        } else {
                            Text("Untitled")
                        }
                        if isSyncing {
                            Label("Syncing", systemImage: "arrow.triangle.2.circlepath")
                                .font(.caption2)
                                .foregroundColor(.secondary)
                        }
                        if let inlineErrorMessage {
                            Text(inlineErrorMessage)
                                .lineLimit(1)
                                .foregroundColor(.red)
                        }
                        Spacer()
                        Text(client.connectionStatus.rawValue)
                            .foregroundColor(client.isConnected ? .green : .orange)
                        if let version = client.coreVersion {
                            Text("Core v\(version)")
                        }
                    }
                    .font(.caption)
                    .padding(.horizontal)
                    .frame(height: 24)
                    .background(Color(NSColor.windowBackgroundColor))
                    .overlay(
                        Rectangle()
                            .frame(height: 1)
                            .foregroundColor(Color(NSColor.separatorColor)),
                        alignment: .top
                    )
                }
            }
        }
        .task {
            await connectAndLoad()
        }
        .onDisappear {
            ghostTask?.cancel()
            let pendingEdit = editPipeline
            Task {
                await pendingEdit?.value
                await closeViewIfNeeded()
            }
        }
    }

    private func connectAndLoad() async {
        isLoading = true
        errorMessage = nil
        inlineErrorMessage = nil
        ghostText = nil

        if !client.isConnected {
            await client.connect()
        }

        guard client.isConnected else {
            errorMessage = client.lastError ?? "Failed to connect to Core"
            isLoading = false
            return
        }

        do {
            let (id, initialContent) = try await client.newView(path: filePath)
            viewId = id
            text = initialContent
            spans = []
            cursorByteOffset = initialContent.utf8.count

            if let path = filePath {
                try? await client.indexFile(path: path)
            }

            isLoading = false
        } catch {
            errorMessage = error.localizedDescription
            isLoading = false
        }
    }

    private func closeViewIfNeeded() async {
        guard let id = viewId else { return }
        try? await client.closeView(viewId: id)
        viewId = nil
    }

    private func handleTextChange(previousText: String, newText: String, cursorByteOffset: Int) {
        self.cursorByteOffset = cursorByteOffset
        text = newText
        ghostText = nil
        latestEditVersion &+= 1
        let editVersion = latestEditVersion

        if let id = viewId, let edit = computeEdit(from: previousText, to: newText) {
            let previousTask = editPipeline
            let baseContent = previousText
            editPipeline = Task {
                await MainActor.run {
                    isSyncing = true
                    inlineErrorMessage = nil
                }
                await previousTask?.value
                do {
                    let authoritativeText = try await client.applyEdit(
                        viewId: id,
                        startByteOffset: edit.startByteOffset,
                        deletedByteCount: edit.deletedByteCount,
                        insertedText: edit.insertedText,
                        baseContent: baseContent
                    )
                    await MainActor.run {
                        guard editVersion == latestEditVersion else { return }
                        if text != authoritativeText {
                            text = authoritativeText
                        }
                        isSyncing = false
                    }
                } catch {
                    await MainActor.run {
                        inlineErrorMessage = error.localizedDescription
                        isSyncing = false
                    }
                }
            }
        }

        scheduleGhostTextRequest(position: cursorByteOffset)
    }

    private func scheduleGhostTextRequest(position: Int) {
        ghostTask?.cancel()
        guard let id = viewId else { return }

        ghostTask = Task {
            try? await Task.sleep(nanoseconds: 500_000_000)
            guard !Task.isCancelled else { return }

            if let ghost = try? await client.requestCompletion(viewId: id, position: position) {
                await MainActor.run {
                    ghostText = ghost
                }
            }
        }
    }

    private func computeEdit(from oldText: String, to newText: String) -> TextEdit? {
        if oldText == newText {
            return nil
        }

        let oldScalars = oldText.unicodeScalars
        let newScalars = newText.unicodeScalars

        var oldStart = oldScalars.startIndex
        var newStart = newScalars.startIndex
        while oldStart < oldScalars.endIndex &&
              newStart < newScalars.endIndex &&
              oldScalars[oldStart] == newScalars[newStart] {
            oldStart = oldScalars.index(after: oldStart)
            newStart = newScalars.index(after: newStart)
        }

        var oldEnd = oldScalars.endIndex
        var newEnd = newScalars.endIndex
        while oldEnd > oldStart && newEnd > newStart {
            let oldPrevious = oldScalars.index(before: oldEnd)
            let newPrevious = newScalars.index(before: newEnd)
            if oldScalars[oldPrevious] != newScalars[newPrevious] {
                break
            }
            oldEnd = oldPrevious
            newEnd = newPrevious
        }

        let deletedByteCount = oldText[oldStart..<oldEnd].utf8.count
        let insertedText = String(newScalars[newStart..<newEnd])
        if deletedByteCount == 0 && insertedText.isEmpty {
            return nil
        }

        let startByteOffset = oldText[..<oldStart].utf8.count
        return TextEdit(
            startByteOffset: startByteOffset,
            deletedByteCount: deletedByteCount,
            insertedText: insertedText
        )
    }
}

private struct TextEdit {
    let startByteOffset: Int
    let deletedByteCount: Int
    let insertedText: String
}

/// Native NSTextView wrapper for SwiftUI
struct NativeTextEditor: NSViewRepresentable {
    @Binding var text: String
    @Binding var spans: [HighlightSpan]
    var onTextChange: (_ previousText: String, _ newText: String, _ cursorByteOffset: Int) -> Void
    var onSelectionChange: (_ cursorByteOffset: Int) -> Void

    func makeNSView(context: Context) -> NSScrollView {
        let scrollView = NSTextView.scrollableTextView()
        guard let textView = scrollView.documentView as? NSTextView else {
            return scrollView
        }

        textView.delegate = context.coordinator
        textView.isRichText = true
        textView.font = NSFont.monospacedSystemFont(ofSize: 13, weight: .regular)
        textView.backgroundColor = NSColor.textBackgroundColor
        textView.textColor = NSColor.textColor
        textView.allowsUndo = true
        return scrollView
    }

    func updateNSView(_ scrollView: NSScrollView, context: Context) {
        guard let textView = scrollView.documentView as? NSTextView else { return }

        let shouldUpdateText = textView.string != text
        if shouldUpdateText {
            let previousSelection = textView.selectedRange()
            context.coordinator.isApplyingProgrammaticChange = true
            textView.string = text

            let stringLength = (textView.string as NSString).length
            let selectionLocation = min(max(previousSelection.location, 0), stringLength)
            textView.setSelectedRange(NSRange(location: selectionLocation, length: 0))

            context.coordinator.isApplyingProgrammaticChange = false
            applyHighlights(to: textView.textStorage)
            context.coordinator.lastSpans = spans
        } else if context.coordinator.lastSpans != spans {
            applyHighlights(to: textView.textStorage)
            context.coordinator.lastSpans = spans
        }
    }

    private func applyHighlights(to storage: NSTextStorage?) {
        guard let storage else { return }
        let sourceText = storage.string
        storage.setAttributes(
            [
                .font: NSFont.monospacedSystemFont(ofSize: 13, weight: .regular),
                .foregroundColor: NSColor.textColor
            ],
            range: NSRange(location: 0, length: storage.length)
        )

        for span in spans {
            guard let start = utf16Offset(forByteOffset: span.start, in: sourceText),
                  let end = utf16Offset(forByteOffset: span.end, in: sourceText),
                  end >= start else {
                continue
            }

            let range = NSRange(location: start, length: end - start)
            guard range.upperBound <= storage.length else { continue }

            var color = NSColor.textColor
            switch span.highlight {
            case "keyword": color = NSColor.systemPurple
            case "string": color = NSColor.systemGreen
            case "comment": color = NSColor.systemGray
            case "function": color = NSColor.systemBlue
            case "type": color = NSColor.systemYellow
            case "number": color = NSColor.systemOrange
            default: break
            }
            storage.addAttribute(.foregroundColor, value: color, range: range)
        }
    }

    private func utf16Offset(forByteOffset byteOffset: Int, in string: String) -> Int? {
        guard byteOffset >= 0 && byteOffset <= string.utf8.count else { return nil }
        let utf8View = string.utf8
        let utf8Index = utf8View.index(utf8View.startIndex, offsetBy: byteOffset)
        guard let index = String.Index(utf8Index, within: string) else {
            return nil
        }
        return index.utf16Offset(in: string)
    }

    func makeCoordinator() -> Coordinator {
        Coordinator(text: $text, onTextChange: onTextChange, onSelectionChange: onSelectionChange)
    }

    class Coordinator: NSObject, NSTextViewDelegate {
        var text: Binding<String>
        var onTextChange: (_ previousText: String, _ newText: String, _ cursorByteOffset: Int) -> Void
        var onSelectionChange: (_ cursorByteOffset: Int) -> Void
        var lastSpans: [HighlightSpan] = []
        var isApplyingProgrammaticChange = false

        init(
            text: Binding<String>,
            onTextChange: @escaping (_ previousText: String, _ newText: String, _ cursorByteOffset: Int) -> Void,
            onSelectionChange: @escaping (_ cursorByteOffset: Int) -> Void
        ) {
            self.text = text
            self.onTextChange = onTextChange
            self.onSelectionChange = onSelectionChange
        }

        func textDidChange(_ notification: Notification) {
            guard let textView = notification.object as? NSTextView else { return }

            let previousText = text.wrappedValue
            let newText = textView.string
            text.wrappedValue = newText

            if isApplyingProgrammaticChange {
                return
            }

            let cursorByteOffset = Self.byteOffset(forUTF16Location: textView.selectedRange().location, in: newText)
            onTextChange(previousText, newText, cursorByteOffset)
        }

        func textViewDidChangeSelection(_ notification: Notification) {
            guard !isApplyingProgrammaticChange,
                  let textView = notification.object as? NSTextView else {
                return
            }
            let cursorByteOffset = Self.byteOffset(forUTF16Location: textView.selectedRange().location, in: textView.string)
            onSelectionChange(cursorByteOffset)
        }

        private static func byteOffset(forUTF16Location location: Int, in string: String) -> Int {
            let utf16Count = string.utf16.count
            let clampedLocation = min(max(location, 0), utf16Count)
            let utf16Index = string.utf16.index(string.utf16.startIndex, offsetBy: clampedLocation)
            guard let scalarIndex = String.Index(utf16Index, within: string) else {
                return string.utf8.count
            }
            return string[..<scalarIndex].utf8.count
        }
    }
}
