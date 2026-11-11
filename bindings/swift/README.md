# Swift binding

> **Status: 📐 design** — the package and the XCFramework build exist, and CI builds the framework on a macOS
> runner. Nothing has been linked into an app or run on an iPhone or the Simulator yet. There is no Swift
> wrapper: `import Mediaway` re-exports the C ABI (`CMediaway`).

Build the framework on a Mac, then depend on this package by path:

```sh
tools/scripts/build-xcframework.sh                  # target/xcframework/Mediaway.xcframework (+ .zip, .sha256)
MEDIAWAY_XCF_MACOS=1 tools/scripts/build-xcframework.sh   # also a macOS (Apple Silicon) slice
```

In Xcode: File > Add Package Dependencies > Add Local, pick `bindings/swift`. Package.swift links the Apple
frameworks the static library calls (a static library does not carry them).

What runs on an iPhone is the Apple backends in `mediaway-encoder::apple`, `mediaway-decoder::apple` and
`mediaway-device::apple` (VideoToolbox, AudioToolbox, AVFoundation, ReplayKit). Their state, which of them has
ever run on a real Mac, and the permissions an app must declare are in
[`docs/contributing/ios-on-a-mac.md`](../../docs/contributing/ios-on-a-mac.md).

The C surface is the same as every other binding's: [`../c/README.md`](../c/README.md).
