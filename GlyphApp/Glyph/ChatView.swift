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
    @State private var messages: [ChatMessage] = []
    @State private var inputText: String = ""
    @State private var isTyping = false

    private var canSend: Bool {
        client.isConnected &&
        !inputText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty &&
        !isTyping
    }

    var body: some View {
        VStack(spacing: 0) {
            // Header
            HStack {
                Text("AI Assistant")
                    .font(.headline)
                Text(client.isConnected ? "Connected" : "Disconnected")
                    .font(.caption)
                    .padding(.horizontal, 8)
                    .padding(.vertical, 2)
                    .background(client.isConnected ? Color.green.opacity(0.2) : Color.orange.opacity(0.2))
                    .clipShape(Capsule())
                Spacer()
                if isTyping {
                    ProgressView()
                        .scaleEffect(0.5)
                }
            }
            .padding()
            .background(Color(NSColor.controlBackgroundColor))

            if let error = client.lastError, !error.isEmpty {
                Text(error)
                    .font(.caption)
                    .foregroundColor(.red)
                    .lineLimit(2)
                    .padding(.horizontal, 12)
                    .padding(.bottom, 6)
            }

            // Message List
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 12) {
                        ForEach(messages) { msg in
                            ChatBubble(message: msg)
                        }
                    }
                    .padding()
                }
                .onChange(of: messages.count) { _, _ in
                    if let last = messages.last {
                        withAnimation {
                            proxy.scrollTo(last.id, anchor: .bottom)
                        }
                    }
                }
            }

            // Input Area
            VStack(spacing: 8) {
                HStack(alignment: .bottom, spacing: 8) {
                    TextEditor(text: $inputText)
                        .font(.body)
                        .frame(minHeight: 40, maxHeight: 100)
                        .fixedSize(horizontal: false, vertical: true)
                        .padding(4)
                        .background(Color(NSColor.textBackgroundColor))
                        .cornerRadius(8)
                        .overlay(
                            RoundedRectangle(cornerRadius: 8)
                                .stroke(Color.secondary.opacity(0.2), lineWidth: 1)
                        )
                        .disabled(!client.isConnected || isTyping)

                    Button(action: sendMessage) {
                        Image(systemName: "arrow.up.circle.fill")
                            .font(.system(size: 24))
                    }
                    .buttonStyle(.plain)
                    .foregroundColor(canSend ? .accentColor : .secondary)
                    .disabled(!canSend)
                }
            }
            .padding()
            .background(Color(NSColor.controlBackgroundColor))
        }
        .task {
            if !client.isConnected {
                await client.connect()
            }
        }
    }

    private func sendMessage() {
        guard canSend else { return }

        let userMsg = inputText
        messages.append(ChatMessage(text: userMsg, isUser: true))
        inputText = ""
        isTyping = true

        Task {
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

                // Mock context
                let responseText = try await client.chat(userMsg, contextFiles: [])

                // Simulate typing effect
                await MainActor.run {
                    messages.append(ChatMessage(text: "", isUser: false))
                }

                var currentText = ""
                for char in responseText {
                    try await Task.sleep(nanoseconds: 20_000_000) // 20ms delay
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
        HStack {
            if message.isUser {
                Spacer()
            }

            Text(message.text)
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
