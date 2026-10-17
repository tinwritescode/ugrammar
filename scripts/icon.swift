// Renders src-tauri/icons/icon.svg into icon.icns for the bundle and icon.png for the window icon.
import AppKit

let dir = URL(fileURLWithPath: "src-tauri/icons")
guard let image = NSImage(contentsOf: dir.appendingPathComponent("icon.svg")) else { fatalError("cannot read icon.svg") }

func png(_ side: Int, to url: URL) {
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: side, pixelsHigh: side, bitsPerSample: 8,
                               samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
                               bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    image.draw(in: NSRect(x: 0, y: 0, width: side, height: side))
    NSGraphicsContext.restoreGraphicsState()
    try! rep.representation(using: .png, properties: [:])!.write(to: url)
}

let iconset = FileManager.default.temporaryDirectory.appendingPathComponent("ugrammar.iconset")
try? FileManager.default.removeItem(at: iconset)
try! FileManager.default.createDirectory(at: iconset, withIntermediateDirectories: true)
for side in [16, 32, 128, 256, 512] {
    png(side, to: iconset.appendingPathComponent("icon_\(side)x\(side).png"))
    png(side * 2, to: iconset.appendingPathComponent("icon_\(side)x\(side)@2x.png"))
}
let iconutil = Process()
iconutil.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
iconutil.arguments = ["-c", "icns", iconset.path, "-o", dir.appendingPathComponent("icon.icns").path]
try! iconutil.run()
iconutil.waitUntilExit()
precondition(iconutil.terminationStatus == 0, "iconutil failed")
png(512, to: dir.appendingPathComponent("icon.png"))
