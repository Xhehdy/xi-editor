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

    var body: some View {
        List {
            FileNodeView(path: rootPath,
                        isRoot: true,
                        selectedPath: $selectedPath,
                        expandedPaths: $expandedPaths)
        }
        .listStyle(.sidebar)
    }
}

struct FileNodeView: View {
    let path: String
    let isRoot: Bool
    @Binding var selectedPath: String?
    @Binding var expandedPaths: Set<String>

    @State private var children: [String]?

    var isExpanded: Bool {
        expandedPaths.contains(path)
    }

    var body: some View {
        if isRoot {
            // Don't show root itself, just children (or show root as header?)
            // Usually we show the folder name if it's a project
            if let children = children {
                ForEach(children, id: \.self) { childPath in
                    FileNodeView(path: childPath, isRoot: false, selectedPath: $selectedPath, expandedPaths: $expandedPaths)
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
                        Text(URL(fileURLWithPath: path).lastPathComponent)
                            .foregroundColor(selectedPath == path ? .accentColor : .primary)
                    } icon: {
                        Image(systemName: iconName)
                            .foregroundColor(isDirectory ? .blue : .secondary)
                    }
                    .font(.callout) // Consistent sizing

                    Spacer()
                }
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
                    ForEach(children, id: \.self) { childPath in
                        FileNodeView(path: childPath, isRoot: false, selectedPath: $selectedPath, expandedPaths: $expandedPaths)
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
            children = items.map { (path as NSString).appendingPathComponent($0) }
                .sorted { p1, p2 in
                    // Folders first
                    let d1 = dirCheck(p1)
                    let d2 = dirCheck(p2)
                    if d1 != d2 { return d1 }
                    return p1 < p2
                }
        } catch {
            print("Failed to list directory: \(error)")
        }
    }

    private func dirCheck(_ p: String) -> Bool {
        var isDir: ObjCBool = false
        FileManager.default.fileExists(atPath: p, isDirectory: &isDir)
        return isDir.boolValue
    }
}
