# iOS on a Mac

What is ready for iOS, what has never been run, and the steps to take on a Mac. Written without a Mac: every
"works" below was measured by cross-compiling (`cargo check/clippy --target aarch64-apple-ios`) or by the
macOS CI runner, and none by running on a device or the Simulator.

## Where things stand

| Piece | State |
|---|---|
| `mediaway-encoder`, `-decoder`, `-device`, `mediaway`, `mediaway-container`, `mediaway-sw`, `mediaway-ffi` | Compile for `aarch64-apple-ios` (checked 2026-11-11; CI lints the first three, the umbrella and the C ABI) |
| Simulator target `aarch64-apple-ios-sim` | `cargo check` in CI |
| AAC (AudioToolbox) | Ran on a macOS runner (v0.2.1). Not on iOS. |
| VideoToolbox H.264/HEVC/VP9/AV1/ProRes, camera, microphone, ReplayKit | Authored, compile-verified only. v0.2.1 found two defects in the one Apple path that did run: expect more. |
| `Mediaway.xcframework` and `bindings/swift` | Built in CI (`apple-xcframework`), never linked into an app |

## One-time setup

1. Xcode from the App Store, then `sudo xcode-select -s /Applications/Xcode.app` and `xcodebuild -runFirstLaunch`.
2. Rust 1.98 through rustup, then `rustup target add aarch64-apple-ios aarch64-apple-ios-sim`.
3. `cargo install cargo-nextest --locked`.
4. An Apple ID in Xcode (Settings > Accounts) for a free development certificate. A real iPhone needs Developer
   Mode (Settings > Privacy & Security) and to be trusted once.

## Build and cross-check

```sh
cargo clippy --target aarch64-apple-ios -p mediaway -p mediaway-ffi --all-features --all-targets -- -D warnings
tools/scripts/build-xcframework.sh
```

## Run the tests on the Simulator

Rust test binaries are plain executables; `simctl spawn` runs them inside a booted Simulator:

```sh
xcrun simctl list devices available   # pick one, then boot it
xcrun simctl boot "<name of an iPhone>"
export CARGO_TARGET_AARCH64_APPLE_IOS_SIM_RUNNER="xcrun simctl spawn booted"
cargo test --target aarch64-apple-ios-sim -p mediaway-decoder --lib apple:: -- --nocapture --test-threads=1
cargo test --target aarch64-apple-ios-sim -p mediaway-encoder --lib apple:: -- --nocapture --test-threads=1
```

The Simulator has no camera and its VideoToolbox is the host's, so hardware paths are not a fair test of a phone;
run the same tests on a device with `cargo-dinghy` or from an Xcode test target. A failing test is data: send the
`OSStatus` lines.

## What an app must declare

- `NSCameraUsageDescription` and `NSMicrophoneUsageDescription` in Info.plist for `mediaway-device::apple`
  camera and microphone capture. Without them iOS ends the app at the first request.
- Screen capture of other apps is a Broadcast Upload Extension, a separate `.appex` target this repository
  cannot build: `crates/mediaway-device/adr/apple/0004-replaykit-ios-inapp-screen-capture.md` is the contract.
  In-app capture (`AppleScreenCapture`) needs no extension.
- A `.xcframework` is linked statically: Package.swift lists the frameworks. If the linker reports an undefined
  symbol for an Apple API, add that framework there and note it here.

## Open on iOS

- `ScreenCaptureKit` is macOS only: the iOS build has no desktop capture, only ReplayKit.
- The Metal side (gway) has no iOS platform crate yet: see gway `docs/adr/` for the iOS platform ADR.
