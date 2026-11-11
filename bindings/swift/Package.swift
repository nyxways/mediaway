// swift-tools-version: 5.9
//
// Mediaway for Swift: the C ABI (`mediaway-ffi`) as a binary target. Build the framework first on a Mac:
//
//     tools/scripts/build-xcframework.sh        # writes target/xcframework/Mediaway.xcframework
//
// then depend on this package by path. `import CMediaway` gives the C functions; there is no Swift wrapper yet
// (bindings/swift/README.md).
import PackageDescription

let package = Package(
    name: "Mediaway",
    platforms: [.iOS(.v16), .macOS(.v13)],
    products: [
        .library(name: "Mediaway", targets: ["Mediaway"]),
    ],
    targets: [
        .binaryTarget(
            name: "CMediaway",
            path: "../../target/xcframework/Mediaway.xcframework"
        ),
        // A static library carries no frameworks, so the ones the encoder, decoder and device code call
        // are linked here. ScreenCaptureKit is macOS only.
        .target(
            name: "Mediaway",
            dependencies: ["CMediaway"],
            path: "Sources/Mediaway",
            linkerSettings: [
                .linkedFramework("Foundation"),
                .linkedFramework("CoreFoundation"),
                .linkedFramework("CoreGraphics"),
                .linkedFramework("CoreMedia"),
                .linkedFramework("CoreVideo"),
                .linkedFramework("CoreAudio"),
                .linkedFramework("AudioToolbox"),
                .linkedFramework("AVFoundation"),
                .linkedFramework("AVFAudio"),
                .linkedFramework("VideoToolbox"),
                .linkedFramework("ReplayKit", .when(platforms: [.iOS])),
                .linkedFramework("ScreenCaptureKit", .when(platforms: [.macOS])),
                .linkedLibrary("objc"),
            ]
        ),
    ]
)
