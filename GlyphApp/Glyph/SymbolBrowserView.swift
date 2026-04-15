//
//  SymbolBrowserView.swift
//  Glyph
//
//  Created for Glyph IDE
//

import SwiftUI
import AppKit

struct SymbolBrowserView: View {
    @ObservedObject var client: GlyphClient
    @State private var searchText = ""
    @State private var symbols: [SymbolInfo] = []
    @State private var isSearching = false
    @State private var errorMessage: String?
    @State private var searchTask: Task<Void, Never>?
    @State private var didApplyLaunchConfiguration = false

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Image(systemName: "magnifyingglass")
                    .foregroundColor(.secondary)
                TextField("Search symbols...", text: $searchText)
                    .textFieldStyle(PlainTextFieldStyle())
                    .accessibilityIdentifier("symbols.search")
                    .onSubmit {
                        scheduleSearch(immediate: true)
                    }

                if !searchText.isEmpty {
                    Button(action: {
                        searchText = ""
                        symbols = []
                        errorMessage = nil
                    }) {
                        Image(systemName: "xmark.circle.fill")
                            .foregroundColor(.secondary)
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(GlyphUI.Space.s10)
            .background(Color(NSColor.controlBackgroundColor))
            .onChange(of: searchText) { _, _ in
                scheduleSearch(immediate: false)
            }

            if isSearching {
                VStack(spacing: GlyphUI.Space.s10) {
                    ProgressView()
                    Text("Searching symbols...")
                        .font(.caption)
                        .foregroundColor(.secondary)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if let errorMessage {
                VStack(spacing: GlyphUI.Space.s10) {
                    Image(systemName: "exclamationmark.triangle")
                        .foregroundColor(.orange)
                    Text(errorMessage)
                        .font(.caption)
                        .multilineTextAlignment(.center)
                        .foregroundColor(.secondary)
                }
                .padding(GlyphUI.Space.s16)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if symbols.isEmpty {
                VStack(spacing: GlyphUI.Space.s8) {
                    Image(systemName: "magnifyingglass.circle")
                        .font(.system(size: 28))
                        .foregroundColor(.secondary)
                    Text(searchText.isEmpty ? "Search indexed symbols" : "No symbols found")
                        .font(.subheadline)
                        .foregroundColor(.secondary)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                List(symbols) { symbol in
                    HStack {
                        iconForSymbol(kind: symbol.kind)
                            .foregroundColor(.accentColor)

                        VStack(alignment: .leading) {
                            Text(symbol.name)
                                .font(.system(.body, design: .monospaced))

                            HStack {
                                Text(symbol.file)
                                    .font(.caption)
                                    .foregroundColor(.secondary)
                                Text(":")
                                    .font(.caption)
                                    .foregroundColor(.secondary)
                                Text("\(symbol.line)")
                                    .font(.caption)
                                    .foregroundColor(.secondary)
                            }
                        }

                        Spacer()

                        if let signature = symbol.signature {
                            Text(signature)
                                .font(.caption)
                                .foregroundColor(.secondary)
                                .lineLimit(1)
                        }
                    }
                    .padding(.vertical, GlyphUI.Space.s4)
                }
                .accessibilityIdentifier("symbols.list")
            }
        }
        .task {
            if shouldUseUITestStubSymbols {
                return
            }
            if !client.isConnected {
                await client.connect()
            }
        }
        .onAppear {
            applyLaunchConfigurationIfNeeded()
        }
        .onDisappear {
            searchTask?.cancel()
        }
        .accessibilityIdentifier("symbols.root")
    }

    private func scheduleSearch(immediate: Bool) {
        searchTask?.cancel()
        let query = searchText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !query.isEmpty else {
            symbols = []
            errorMessage = nil
            isSearching = false
            return
        }

        searchTask = Task {
            if !immediate {
                try? await Task.sleep(nanoseconds: 250_000_000)
                guard !Task.isCancelled else { return }
            }
            await performSearch(query: query)
        }
    }

    @MainActor
    private func performSearch(query: String) async {
        isSearching = true
        errorMessage = nil
        if shouldUseUITestStubSymbols {
            symbols = stubSymbols.filter {
                query.isEmpty || $0.name.localizedCaseInsensitiveContains(query)
            }
            isSearching = false
            return
        }
        do {
            symbols = try await client.searchSymbols(query: query)
        } catch {
            symbols = []
            errorMessage = error.localizedDescription
        }
        isSearching = false
    }

    private func iconForSymbol(kind: String) -> Image {
        switch kind.lowercased() {
        case "function", "method": return Image(systemName: "f.cursive.circle")
        case "struct", "class": return Image(systemName: "s.circle")
        case "enum": return Image(systemName: "e.circle")
        case "trait", "interface": return Image(systemName: "t.circle")
        case "const", "constant": return Image(systemName: "c.circle")
        default: return Image(systemName: "questionmark.circle")
        }
    }

    private var shouldUseUITestStubSymbols: Bool {
        ProcessInfo.processInfo.environment["GLYPH_UI_TEST_STUB_SYMBOLS"] == "1"
    }

    private var stubSymbols: [SymbolInfo] {
        [
            SymbolInfo(
                name: "applyEdit",
                kind: "function",
                file: "glyph-core/src/editor.rs",
                line: 42,
                signature: "fn apply_edit(view_id, patch)"
            ),
            SymbolInfo(
                name: "AccountLedger",
                kind: "struct",
                file: "glyph-core/src/ledger.rs",
                line: 11,
                signature: "struct AccountLedger"
            )
        ]
    }

    private func applyLaunchConfigurationIfNeeded() {
        guard !didApplyLaunchConfiguration else { return }
        didApplyLaunchConfiguration = true

        let env = ProcessInfo.processInfo.environment
        if let initialQuery = env["GLYPH_TEST_SYMBOL_QUERY"], !initialQuery.isEmpty {
            searchText = initialQuery
            scheduleSearch(immediate: true)
        }
    }
}
