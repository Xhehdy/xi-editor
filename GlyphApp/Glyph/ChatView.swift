//
//  ChatView.swift
//  Glyph
//
//  AI Chat Interface
//

import SwiftUI

struct ChatMessage: Identifiable, Equatable {
    let id: UUID
    var text: String
    let isUser: Bool
    let timestamp: Date

    init(id: UUID = UUID(), text: String, isUser: Bool, timestamp: Date = Date()) {
        self.id = id
        self.text = text
        self.isUser = isUser
        self.timestamp = timestamp
    }
}

struct ChatView: View {
    @ObservedObject var client: GlyphClient
    let showsInlineHeader: Bool
    let contextFiles: [String]
    @State private var messages: [ChatMessage] = []
    @State private var inputText: String = ""
    @State private var isTyping = false
    @State private var hasShownWelcome = false

    private var canSend: Bool {
        !inputText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty &&
        !isTyping
    }

    init(client: GlyphClient, showsInlineHeader: Bool = true, contextFiles: [String] = []) {
        self.client = client
        self.showsInlineHeader = showsInlineHeader
        self.contextFiles = contextFiles
    }

    var body: some View {
        VStack(spacing: 0) {
            if showsInlineHeader {
                HStack {
                    Label("Assistant", systemImage: "sparkles")
                        .font(.headline)
                        .accessibilityIdentifier("chat.header")
                    Text(client.connectionStatus.rawValue)
                        .font(.caption)
                        .padding(.horizontal, 8)
                        .padding(.vertical, 2)
                        .background(client.isConnected ? Color.green.opacity(0.2) : Color.orange.opacity(0.2))
                        .clipShape(Capsule())
                    Spacer()
                    if !messages.isEmpty {
                        Button("Clear") {
                            messages.removeAll()
                            client.clearGraphContext()
                            hasShownWelcome = false
                            showWelcomeMessageIfNeeded()
                        }
                        .buttonStyle(.borderless)
                        .font(.caption)
                    }
                    if isTyping {
                        ProgressView()
                            .scaleEffect(0.5)
                    }
                }
                .padding()
                .background(Color(NSColor.controlBackgroundColor))
                .accessibilityIdentifier("chat.toolbar")
            }

            if let error = client.lastError, !error.isEmpty {
                Text(error)
                    .font(.caption)
                    .foregroundColor(.red)
                    .lineLimit(2)
                    .padding(.horizontal, 12)
                    .padding(.bottom, 6)
            }
            if let context = client.lastGraphContext {
                Text("CKG: \(context.items.count) items, \(context.summary)")
                    .font(.caption2)
                    .foregroundColor(.secondary)
                    .lineLimit(2)
                    .padding(.horizontal, 12)
                    .padding(.bottom, 6)
            }

            ScrollViewReader { proxy in
                ScrollView {
                    if messages.isEmpty {
                        emptyState
                            .padding(20)
                    } else {
                        LazyVStack(alignment: .leading, spacing: 12) {
                            ForEach(messages) { msg in
                                ChatBubble(message: msg)
                                    .id(msg.id)
                            }
                        }
                        .padding()
                    }
                }
                .onChange(of: messages.count) { _, _ in
                    if let last = messages.last {
                        withAnimation {
                            proxy.scrollTo(last.id, anchor: .bottom)
                        }
                    }
                }
            }

            VStack(spacing: 8) {
                HStack(alignment: .bottom, spacing: 8) {
                    ZStack(alignment: .topLeading) {
                        if inputText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                            Text("Ask about this project, architecture, or a file...")
                                .foregroundColor(.secondary)
                                .padding(.top, 12)
                                .padding(.leading, 9)
                        }
                        TextEditor(text: $inputText)
                            .font(.body)
                            .frame(minHeight: 40, maxHeight: 110)
                            .fixedSize(horizontal: false, vertical: true)
                            .padding(4)
                            .background(Color(NSColor.textBackgroundColor))
                            .cornerRadius(8)
                            .overlay(
                                RoundedRectangle(cornerRadius: 8)
                                    .stroke(Color.secondary.opacity(0.2), lineWidth: 1)
                            )
                            .disabled(isTyping)
                            .accessibilityIdentifier("chat.input")
                    }

                    Button(action: sendMessage) {
                        Image(systemName: "arrow.up.circle.fill")
                            .font(.system(size: 24))
                    }
                    .buttonStyle(.plain)
                    .foregroundColor(canSend ? .accentColor : .secondary)
                    .disabled(!canSend)
                    .keyboardShortcut(.return, modifiers: [.command])
                    .help("Send (Command + Return)")
                }
            }
            .padding()
            .background(Color(NSColor.controlBackgroundColor))
        }
        .task {
            if shouldUseUITestStubChat {
                showWelcomeMessageIfNeeded()
                return
            }
            if !client.isConnected {
                await client.connect()
            }
            showWelcomeMessageIfNeeded()
        }
        .accessibilityIdentifier("chat.root")
    }

    private var emptyState: some View {
        VStack(alignment: .leading, spacing: 8) {
            Label("No messages yet", systemImage: "bubble.left.and.bubble.right")
                .font(.headline)
            Text("Start with a question like:")
                .font(.subheadline)
                .foregroundColor(.secondary)
            Text("• Summarize this file\n• Explain this error\n• Suggest a refactor")
                .font(.caption)
                .foregroundColor(.secondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(12)
        .background(Color(NSColor.controlBackgroundColor))
        .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
    }

    private func showWelcomeMessageIfNeeded() {
        guard !hasShownWelcome else { return }
        messages.append(
            ChatMessage(
                text: shouldUseUITestStubChat
                    ? "Stub assistant ready for UI snapshots."
                    : "I can help with this codebase. Ask about files, architecture, or bugs.",
                isUser: false
            )
        )
        hasShownWelcome = true
    }

    private func sendMessage() {
        guard canSend else { return }

        let userMsg = inputText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !userMsg.isEmpty else { return }

        messages.append(ChatMessage(text: userMsg, isUser: true))
        inputText = ""
        isTyping = true

        Task {
            if shouldUseUITestStubChat {
                await MainActor.run {
                    messages.append(ChatMessage(text: "Stub response: \(userMsg)", isUser: false))
                    isTyping = false
                }
                return
            }

            do {
                if !client.isConnected {
                    await client.connect()
                }
                guard client.isConnected else {
                    await MainActor.run {
                        messages.append(ChatMessage(text: "Error: Core is not connected.", isUser: false))
                        isTyping = false
                    }
                    return
                }

                let response = try await client.chatWithContext(userMsg, contextFiles: contextFiles)
                let responseText = response.text
                await MainActor.run {
                    messages.append(ChatMessage(text: "", isUser: false))
                }

                var currentText = ""
                for char in responseText {
                    try await Task.sleep(nanoseconds: 18_000_000)
                    currentText.append(char)
                    updateLastMessage(currentText)
                }
                await MainActor.run {
                    isTyping = false
                }
            } catch {
                await MainActor.run {
                    messages.append(ChatMessage(text: "Error: \(error.localizedDescription)", isUser: false))
                    isTyping = false
                }
            }
        }
    }

    private var shouldUseUITestStubChat: Bool {
        ProcessInfo.processInfo.environment["GLYPH_UI_TEST_STUB_CHAT"] == "1"
    }

    @MainActor
    private func updateLastMessage(_ text: String) {
        if !messages.isEmpty {
            let lastIdx = messages.count - 1
            messages[lastIdx].text = text
        }
    }
}

struct ChatBubble: View {
    let message: ChatMessage

    var body: some View {
        HStack(alignment: .bottom) {
            if message.isUser {
                Spacer()
            }

            VStack(alignment: .leading, spacing: 4) {
                Text(message.text)
                    .textSelection(.enabled)
                Text(message.timestamp, style: .time)
                    .font(.caption2)
                    .foregroundColor(message.isUser ? .white.opacity(0.75) : .secondary)
            }
            .padding(10)
            .background(message.isUser ? Color.accentColor : Color(NSColor.windowBackgroundColor))
            .foregroundColor(message.isUser ? .white : .primary)
            .cornerRadius(12)
            .overlay(
                RoundedRectangle(cornerRadius: 12)
                    .stroke(Color.secondary.opacity(0.1), lineWidth: 1)
            )

            if !message.isUser {
                Spacer()
            }
        }
    }
}
