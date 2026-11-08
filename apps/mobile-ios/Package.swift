// swift-tools-version: 6.0
// SirTone for iPhone (stage 8, D-137/D-138): SwiftPM project built by xtool (WSL) and by the
// macOS runner of GitHub Actions. The Rust core arrives as `Frameworks/SirToneCoreFFI.xcframework`
// plus generated Swift bindings (`scripts/build-ios-core.sh`); without them the app still builds
// and says the core is not linked (useful for UI-only work).

import Foundation
import PackageDescription

let here = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
let hasCore = FileManager.default.fileExists(
    atPath: here.appendingPathComponent("Frameworks/SirToneCoreFFI.xcframework").path)
// The visualiser page (F22), built from apps/desktop by `pnpm build:mobile-viz`.
let hasViz = FileManager.default.fileExists(
    atPath: here.appendingPathComponent("Sources/SirTone/Resources/viz/index.html").path)

var targets: [Target] = [
    .target(name: "SirToneDesign", resources: [.copy("Resources/Geologica.ttf")]),
    .target(
        name: "SirTone",
        dependencies: ["SirToneDesign"] + (hasCore ? ["SirToneCore"] : []),
        exclude: hasViz ? [] : ["Resources"],
        resources: hasViz ? [.copy("Resources/viz")] : []
    ),
    .target(name: "SirToneWidget", dependencies: ["SirToneDesign"]),
]
if hasCore {
    targets += [
        .binaryTarget(name: "SirToneCoreFFI", path: "Frameworks/SirToneCoreFFI.xcframework"),
        .target(name: "SirToneCore", dependencies: ["SirToneCoreFFI"]),
    ]
}

let package = Package(
    name: "SirTone",
    defaultLocalization: "ru",
    platforms: [.iOS(.v17), .macOS(.v14)],
    products: [
        .library(name: "SirTone", targets: ["SirTone"]),
        .library(name: "SirToneWidget", targets: ["SirToneWidget"]),
    ],
    targets: targets
)
