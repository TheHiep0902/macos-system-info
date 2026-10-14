#!/usr/bin/env swift
import AppKit

enum IconError: Error {
    case bitmap
    case encode(String)
}

let outputDir = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "src-tauri/icons"

func pulsePath(in rect: NSRect, lineWidth: CGFloat) -> NSBezierPath {
    let path = NSBezierPath()
    let mid = rect.minY + rect.height * 0.5
    let points: [(CGFloat, CGFloat)] = [
        (0.00, mid),
        (0.26, mid),
        (0.38, rect.minY + rect.height * 0.92),
        (0.56, rect.minY + rect.height * 0.08),
        (0.70, mid),
        (1.00, mid),
    ]
    for (index, point) in points.enumerated() {
        let target = NSPoint(x: rect.minX + rect.width * point.0, y: point.1)
        if index == 0 {
            path.move(to: target)
        } else {
            path.line(to: target)
        }
    }
    path.lineWidth = lineWidth
    path.lineCapStyle = .round
    path.lineJoinStyle = .round
    return path
}

func render(pixels: Int, draw: (NSRect) -> Void) throws -> NSBitmapImageRep {
    guard
        let rep = NSBitmapImageRep(
            bitmapDataPlanes: nil,
            pixelsWide: pixels,
            pixelsHigh: pixels,
            bitsPerSample: 8,
            samplesPerPixel: 4,
            hasAlpha: true,
            isPlanar: false,
            colorSpaceName: .deviceRGB,
            bytesPerRow: 0,
            bitsPerPixel: 0
        )
    else {
        throw IconError.bitmap
    }
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    NSGraphicsContext.current?.imageInterpolation = .high
    draw(NSRect(x: 0, y: 0, width: pixels, height: pixels))
    NSGraphicsContext.restoreGraphicsState()
    return rep
}

func write(_ rep: NSBitmapImageRep, name: String) throws {
    guard let data = rep.representation(using: .png, properties: [:]) else {
        throw IconError.encode(name)
    }
    let url = URL(fileURLWithPath: outputDir).appendingPathComponent(name)
    try data.write(to: url)
    print("wrote \(url.path)")
}

func trayTemplate(pixels: Int) throws -> NSBitmapImageRep {
    let scale = CGFloat(pixels) / 18.0
    return try render(pixels: pixels) { bounds in
        NSColor.black.setStroke()
        let inset = bounds.insetBy(dx: 1.5 * scale, dy: 2.5 * scale)
        pulsePath(in: inset, lineWidth: 1.7 * scale).stroke()
    }
}

func appIcon(pixels: Int) throws -> NSBitmapImageRep {
    let unit = CGFloat(pixels) / 1024.0
    return try render(pixels: pixels) { bounds in
        let tile = bounds.insetBy(dx: 100 * unit, dy: 100 * unit)
        let background = NSBezierPath(roundedRect: tile, xRadius: 185 * unit, yRadius: 185 * unit)
        let gradient = NSGradient(
            starting: NSColor(srgbRed: 0.09, green: 0.16, blue: 0.27, alpha: 1),
            ending: NSColor(srgbRed: 0.04, green: 0.42, blue: 0.52, alpha: 1)
        )
        gradient?.draw(in: background, angle: 90)
        NSColor.white.setStroke()
        let glyph = tile.insetBy(dx: 150 * unit, dy: 250 * unit)
        pulsePath(in: glyph, lineWidth: 46 * unit).stroke()
    }
}

do {
    try FileManager.default.createDirectory(
        atPath: outputDir, withIntermediateDirectories: true)
    try write(trayTemplate(pixels: 18), name: "tray-template.png")
    try write(trayTemplate(pixels: 36), name: "tray-template@2x.png")
    try write(appIcon(pixels: 1024), name: "app-icon-1024.png")
} catch {
    FileHandle.standardError.write("gen-icons failed: \(error)\n".data(using: .utf8)!)
    exit(1)
}
