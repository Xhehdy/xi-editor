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
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 1) {
                ForEach(openFiles, id: \.self) { file in
                    TabItemView(
                        path: file,
                        isActive: file == activeFile,
                        onClose: { closeFile(file) },
                        onSelect: { activeFile = file }
                    )
                }
            }
            .padding(.leading, 8)
        }
        .frame(height: 32)
        .background(Color(NSColor.controlBackgroundColor))
        .overlay(
            Rectangle()
                .frame(height: 1)
                .foregroundColor(Color(NSColor.separatorColor)),
            alignment: .bottom
        )
    }

    private func closeFile(_ path: String) {
        if let idx = openFiles.firstIndex(of: path) {
            openFiles.remove(at: idx)
            if activeFile == path {
                if openFiles.isEmpty {
                    activeFile = nil
                } else {
                    activeFile = openFiles[min(idx, openFiles.count - 1)]
                }
            }
        }
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
                .font(.system(size: 12))
                .foregroundColor(.secondary)

            Text(URL(fileURLWithPath: path).lastPathComponent)
                .font(.system(size: 12, weight: isActive ? .medium : .regular))
                .foregroundColor(isActive ? .primary : .secondary)

            if isHovering || isActive {
                Button(action: onClose) {
                    Image(systemName: "xmark")
                        .font(.system(size: 10))
                        .foregroundColor(.secondary)
                }
                .buttonStyle(.plain)
            } else {
                Spacer().frame(width: 10) // Placeholder for xmark
            }
        }
        .padding(.horizontal, 10)
        .frame(height: 28)
        .background(isActive ? Color(NSColor.textBackgroundColor) : Color.clear)
        .cornerRadius(4, corners: [.topLeft, .topRight])
        .contentShape(Rectangle())
        .onTapGesture { onSelect() }
        .onHover { isHovering = $0 }
    }

    private var iconName: String {
        let ext = URL(fileURLWithPath: path).pathExtension.lowercased()
        switch ext {
        case "rs": return "gearshape.fill"
        case "swift": return "swift"
        default: return "doc"
        }
    }
}

// Rounded corner extension
extension View {
    func cornerRadius(_ radius: CGFloat, corners: RectCorner) -> some View {
        clipShape(RoundedCorner(radius: radius, corners: corners))
    }
}

struct RoundedCorner: Shape {
    var radius: CGFloat = .infinity
    var corners: RectCorner = .allCorners

    func path(in rect: CGRect) -> Path {
        #if os(macOS)
        let path = NSBezierPath(
            roundedRect: rect,
            xRadius: radius,
            yRadius: radius
        )
        return Path(path.cgPath)
        #else
        return Path(roundedRect: rect, cornerRadius: radius)
        #endif
    }
}

// Helper to get CGPath from NSBezierPath
extension NSBezierPath {
    var cgPath: CGPath {
        let path = CGMutablePath()
        var points = [CGPoint](repeating: .zero, count: 3)
        for i in 0 ..< self.elementCount {
            let type = self.element(at: i, associatedPoints: &points)
            switch type {
            case .moveTo: path.move(to: points[0])
            case .lineTo: path.addLine(to: points[0])
            case .curveTo, .cubicCurveTo:
                path.addCurve(to: points[2], control1: points[0], control2: points[1])
            case .quadraticCurveTo:
                path.addQuadCurve(to: points[1], control: points[0])
            case .closePath: path.closeSubpath()
            @unknown default: continue
            }
        }
        return path
    }
}

// Removed RectCorner and custom rounding logic for now to keep it simple.
// SwiftUI's .cornerRadius is deprecated but still works, or use .clipShape(RoundedRectangle...)
// But to match specific corners:
// RoundedCornerShape removed (redundant)

// Simple option set for compatibility (mimicking UIRectCorner)
struct RectCorner: OptionSet {
    let rawValue: Int
    static let topLeft = RectCorner(rawValue: 1 << 0)
    static let topRight = RectCorner(rawValue: 1 << 1)
    static let bottomLeft = RectCorner(rawValue: 1 << 2)
    static let bottomRight = RectCorner(rawValue: 1 << 3)
    static let allCorners: RectCorner = [.topLeft, .topRight, .bottomLeft, .bottomRight]
}
