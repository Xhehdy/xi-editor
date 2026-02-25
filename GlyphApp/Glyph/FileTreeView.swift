//
//  FileTreeView.swift
//  Glyph
//
//  Recursive file explorer sidebar
//

import SwiftUI
import AppKit

struct FileTreeView: View {
    let rootPath: String
    @Binding var selectedPath: String?
    @State private var expandedPaths: Set<String> = []
    @State private var showHiddenFiles = false
    @State private var filterText = ""

    var body: some View {
        VStack(spacing: 0) {
            VStack(spacing: 8) {
                HStack {
                    Text(URL(fileURLWithPath: rootPath).lastPathComponent)
                        .font(.caption)
                        .foregroundColor(.secondary)
                        .lineLimit(1)
                    Spacer()
                    Button {
                        showHiddenFiles.toggle()
                    } label: {
                        Image(systemName: showHiddenFiles ? "eye.slash" : "eye")
                    }
                    .buttonStyle(.plain)
                    .help(showHiddenFiles ? "Hide hidden files" : "Show hidden files")
                }

                HStack(spacing: 6) {
                    Image(systemName: "magnifyingglass")
                        .foregroundColor(.secondary)
                    TextField("Filter files", text: $filterText)
                        .textFieldStyle(.plain)
                        .accessibilityIdentifier("files.filter")
                    if !filterText.isEmpty {
                        Button {
                            filterText = ""
                        } label: {
                            Image(systemName: "xmark.circle.fill")
                                .foregroundColor(.secondary)
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(.vertical, 2)
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .overlay(
                Rectangle()
                    .frame(height: 1)
                    .foregroundColor(Color(NSColor.separatorColor)),
                alignment: .bottom
            )

            List {
                FileNodeView(
                    path: rootPath,
                    isRoot: true,
                    selectedPath: $selectedPath,
                    expandedPaths: $expandedPaths,
                    showHiddenFiles: showHiddenFiles,
                    filterText: filterText
                )
                .id("root-\(showHiddenFiles)")
            }
            .listStyle(.sidebar)
            .accessibilityIdentifier("files.list")
        }
        .accessibilityIdentifier("files.root")
    }
}

struct FileNodeView: View {
    let path: String
    let isRoot: Bool
    @Binding var selectedPath: String?
    @Binding var expandedPaths: Set<String>
    let showHiddenFiles: Bool
    let filterText: String

    @State private var children: [String]?

    var isExpanded: Bool {
        expandedPaths.contains(path)
    }

    var body: some View {
        if isRoot {
            // Don't show root itself, just children (or show root as header?)
            // Usually we show the folder name if it's a project
            if let children = children {
                ForEach(filteredChildren(from: children), id: \.self) { childPath in
                    FileNodeView(
                        path: childPath,
                        isRoot: false,
                        selectedPath: $selectedPath,
                        expandedPaths: $expandedPaths,
                        showHiddenFiles: showHiddenFiles,
                        filterText: filterText
                    )
                }
            } else {
                ProgressView()
                    .task { await loadChildren() }
            }
        } else {
            VStack(alignment: .leading, spacing: 0) {
                HStack {
                    if isDirectory {
                        Image(systemName: isExpanded ? "chevron.down" : "chevron.right")
                            .font(.system(size: 10))
                            .frame(width: 12)
                            .onTapGesture { toggleExpand() }
                    } else {
                        Spacer().frame(width: 12)
                    }

                    Label {
                        Text(fileName)
                            .foregroundColor(selectedPath == path ? .accentColor : .primary)
                    } icon: {
                        Image(systemName: iconName)
                            .foregroundColor(isDirectory ? .accentColor : .secondary)
                    }
                    .font(.callout)

                    Spacer()
                }
                .padding(.horizontal, 6)
                .padding(.vertical, 4)
                .contentShape(Rectangle())
                .onTapGesture {
                    if isDirectory {
                        toggleExpand()
                    } else {
                        selectedPath = path
                    }
                }

                if isExpanded, let children = children {
                    ForEach(filteredChildren(from: children), id: \.self) { childPath in
                        FileNodeView(
                            path: childPath,
                            isRoot: false,
                            selectedPath: $selectedPath,
                            expandedPaths: $expandedPaths,
                            showHiddenFiles: showHiddenFiles,
                            filterText: filterText
                        )
                            .padding(.leading, 16)
                    }
                }
            }
            .task {
                if isExpanded && children == nil {
                    await loadChildren()
                }
            }
        }
    }

    private var isDirectory: Bool {
        var isDir: ObjCBool = false
        FileManager.default.fileExists(atPath: path, isDirectory: &isDir)
        return isDir.boolValue
    }

    private var fileName: String {
        URL(fileURLWithPath: path).lastPathComponent
    }

    private var normalizedFilterText: String {
        filterText.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private func filteredChildren(from paths: [String]) -> [String] {
        guard !normalizedFilterText.isEmpty else {
            return paths
        }

        return paths.filter { childPath in
            if dirCheck(childPath) {
                return true
            }
            let fileName = URL(fileURLWithPath: childPath).lastPathComponent
            return fileName.localizedCaseInsensitiveContains(normalizedFilterText)
        }
    }

    private var iconName: String {
        if isDirectory {
            return "folder.fill"
        }
        let ext = URL(fileURLWithPath: path).pathExtension.lowercased()
        switch ext {
        case "rs": return "gearshape.fill" // Rust
        case "swift": return "swift"
        case "json": return "curlybraces"
        case "md": return "doc.text"
        default: return "doc"
        }
    }

    private func toggleExpand() {
        if expandedPaths.contains(path) {
            expandedPaths.remove(path)
        } else {
            expandedPaths.insert(path)
            Task { await loadChildren() }
        }
    }

    private func loadChildren() async {
        guard isDirectory else { return }

        do {
            let items = try FileManager.default.contentsOfDirectory(atPath: path)
            let ignoredNames: Set<String> = [".git", "target", ".build", "DerivedData", "node_modules"]
            let filtered = items.filter { item in
                if ignoredNames.contains(item) {
                    return false
                }
                if !showHiddenFiles && item.hasPrefix(".") {
                    return false
                }
                return true
            }
            children = filtered.map { (path as NSString).appendingPathComponent($0) }
                .sorted { p1, p2 in
                    // Folders first
                    let d1 = dirCheck(p1)
                    let d2 = dirCheck(p2)
                    if d1 != d2 { return d1 }
                    return p1.localizedCaseInsensitiveCompare(p2) == .orderedAscending
                }
        } catch {
            children = []
        }
    }

    private func dirCheck(_ p: String) -> Bool {
        var isDir: ObjCBool = false
        FileManager.default.fileExists(atPath: p, isDirectory: &isDir)
        return isDir.boolValue
    }
}
