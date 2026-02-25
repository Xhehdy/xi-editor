//
//  SymbolBrowserView.swift
//  Glyph
//
//  Created for Glyph IDE
//

import SwiftUI

struct SymbolBrowserView: View {
    @ObservedObject var client: GlyphClient
    @State private var searchText = ""
    @State private var symbols: [SymbolInfo] = []

    var body: some View {
        VStack(spacing: 0) {
            // Search Bar
            HStack {
                Image(systemName: "magnifyingglass")
                    .foregroundColor(.secondary)
                TextField("Search symbols...", text: $searchText)
                    .textFieldStyle(PlainTextFieldStyle())
                    .onSubmit {
                        searchSymbols()
                    }

                if !searchText.isEmpty {
                    Button(action: {
                        searchText = ""
                        symbols = []
                    }) {
                        Image(systemName: "xmark.circle.fill")
                            .foregroundColor(.secondary)
                    }
                }
            }
            .padding(10)
            .background(Color(NSColor.controlBackgroundColor))

            Divider()

            // Symbol List
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
                .padding(.vertical, 4)
            }
        }
    }

    private func searchSymbols() {
        guard !searchText.isEmpty else { return }
        Task {
            do {
                self.symbols = try await client.searchSymbols(query: searchText)
            } catch {
                print("Search failed: \(error)")
            }
        }
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
}
