//
//  TabBarView.swift
//  Glyph
//
//  Top tab bar for open documents
//

import SwiftUI
import AppKit

struct TabBarView: View {
    @Binding var openFiles: [String]
    @Binding var activeFile: String?

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                Label("\(openFiles.count) open", systemImage: "doc.on.doc")
                    .font(.caption)
                    .foregroundColor(.secondary)
                Spacer()
                Button {
                    closeAllFiles()
                } label: {
                    Image(systemName: "xmark.circle")
                        .font(.system(size: 13, weight: .medium))
                        .foregroundColor(.secondary)
                }
                .buttonStyle(.plain)
                .help("Close all tabs")
            }
            .padding(.horizontal, 10)
            .padding(.top, 6)
            .padding(.bottom, 4)

            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 6) {
                    ForEach(openFiles, id: \.self) { file in
                        TabItemView(
                            path: file,
                            isActive: file == activeFile,
                            onClose: { closeFile(file) },
                            onSelect: { activeFile = file }
                        )
                    }
                }
                .padding(.horizontal, 8)
                .padding(.bottom, 6)
            }
        }
        .frame(minHeight: 36)
        .background(Color(NSColor.controlBackgroundColor))
        .overlay(
            Rectangle()
                .frame(height: 1)
                .foregroundColor(Color(NSColor.separatorColor)),
            alignment: .bottom
        )
    }

    private func closeFile(_ path: String) {
        let reducedState = TabBarState.reducedStateAfterClosing(
            openFiles: openFiles,
            activeFile: activeFile,
            closing: path
        )
        openFiles = reducedState.openFiles
        activeFile = reducedState.activeFile
    }

    private func closeAllFiles() {
        openFiles.removeAll()
        activeFile = nil
    }
}

struct TabItemView: View {
    let path: String
    let isActive: Bool
    let onClose: () -> Void
    let onSelect: () -> Void

    @State private var isHovering = false

    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: iconName)
                .font(.system(size: 12, weight: .medium))
                .foregroundColor(iconColor)

            Text(fileName)
                .font(.system(size: 12, weight: isActive ? .medium : .regular))
                .foregroundColor(isActive ? .primary : .secondary)
                .lineLimit(1)
                .truncationMode(.middle)
                .frame(maxWidth: 180, alignment: .leading)

            Button(action: onClose) {
                Image(systemName: "xmark.circle.fill")
                    .font(.system(size: 11, weight: .medium))
                    .foregroundColor(.secondary)
            }
            .buttonStyle(.plain)
            .opacity(isHovering || isActive ? 1.0 : 0.0)
            .allowsHitTesting(isHovering || isActive)
            .accessibilityLabel("Close \(fileName)")
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(backgroundColor)
        .overlay(
            RoundedRectangle(cornerRadius: 6, style: .continuous)
                .stroke(borderColor, lineWidth: 1)
        )
        .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
        .contentShape(Rectangle())
        .onTapGesture { onSelect() }
        .onHover { isHovering = $0 }
        .help(path)
    }

    private var fileName: String {
        URL(fileURLWithPath: path).lastPathComponent
    }

    private var backgroundColor: Color {
        if isActive {
            return Color(NSColor.textBackgroundColor)
        }
        if isHovering {
            return Color(NSColor.windowBackgroundColor).opacity(0.7)
        }
        return Color.clear
    }

    private var borderColor: Color {
        if isActive {
            return Color.accentColor.opacity(0.28)
        }
        return Color(NSColor.separatorColor).opacity(0.3)
    }

    private var iconColor: Color {
        if isActive {
            return Color.accentColor
        }
        return .secondary
    }

    private var iconName: String {
        let ext = URL(fileURLWithPath: path).pathExtension.lowercased()
        switch ext {
        case "md", "txt": return "doc.text"
        case "json", "yml", "yaml", "toml": return "curlybraces"
        case "go": return "g.circle"
        case "rs": return "gearshape.fill"
        case "swift": return "swift"
        case "ex", "exs": return "bolt.circle"
        default: return "doc"
        }
    }
}

enum TabBarState {
    static func reducedStateAfterClosing(
        openFiles: [String],
        activeFile: String?,
        closing path: String
    ) -> (openFiles: [String], activeFile: String?) {
        guard let index = openFiles.firstIndex(of: path) else {
            return (openFiles, activeFile)
        }

        var remainingFiles = openFiles
        remainingFiles.remove(at: index)
        guard activeFile == path else {
            return (remainingFiles, activeFile)
        }

        guard !remainingFiles.isEmpty else {
            return (remainingFiles, nil)
        }

        let nextIndex = min(index, remainingFiles.count - 1)
        return (remainingFiles, remainingFiles[nextIndex])
    }
}
