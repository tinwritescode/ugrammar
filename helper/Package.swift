// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "grammar-helper",
    platforms: [.macOS(.v12)],
    targets: [
        .executableTarget(name: "grammar-helper", path: "Sources/grammar-helper")
    ]
)
