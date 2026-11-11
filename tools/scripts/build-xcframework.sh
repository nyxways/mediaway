#!/usr/bin/env bash
# Build `Mediaway.xcframework` from the `mediaway-ffi` static library: iOS device, iOS Simulator (Apple
# Silicon) and, with MEDIAWAY_XCF_MACOS=1, macOS (Apple Silicon). Runs on macOS with Xcode only.
#
# Usage:
#   tools/scripts/build-xcframework.sh            # release build, output target/xcframework/
#   MEDIAWAY_XCF_PROFILE=debug tools/scripts/build-xcframework.sh
#   MEDIAWAY_XCF_X86_SIM=1 tools/scripts/build-xcframework.sh   # add the Intel Simulator slice (lipo-merged)
#
# Output (target/xcframework/):
#   Mediaway.xcframework       what an Xcode project or bindings/swift/Package.swift links
#   Mediaway.xcframework.zip   the same, zipped, for a SwiftPM `binaryTarget(url:checksum:)`
#   Mediaway.xcframework.zip.sha256
#
# The C headers are `crates/mediaway-ffi/include/mediaway/*.h`, shipped under `Headers/mediaway/`, with a
# module map so Swift can `import CMediaway`. The static library does not carry the frameworks it calls; the
# app (or Package.swift's linkerSettings) links them: see docs/contributing/ios-on-a-mac.md.
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "error: this script needs macOS and Xcode (xcodebuild, lipo)" >&2
  exit 1
fi
command -v xcodebuild >/dev/null || { echo "error: xcodebuild not found; install Xcode and run xcode-select -s" >&2; exit 1; }

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"

profile="${MEDIAWAY_XCF_PROFILE:-release}"
profile_flag="--release"
profile_dir="release"
if [[ "$profile" == "debug" ]]; then
  profile_flag=""
  profile_dir="debug"
fi
out="$root/target/xcframework"
stage="$out/stage"
rm -rf "$out"
mkdir -p "$stage/Headers/mediaway"

targets=(aarch64-apple-ios aarch64-apple-ios-sim)
[[ "${MEDIAWAY_XCF_X86_SIM:-0}" == "1" ]] && targets+=(x86_64-apple-ios)
[[ "${MEDIAWAY_XCF_MACOS:-0}" == "1" ]] && targets+=(aarch64-apple-darwin)

rustup target add "${targets[@]}"

for t in "${targets[@]}"; do
  echo "== cargo build -p mediaway-ffi --target $t ($profile)"
  # shellcheck disable=SC2086
  cargo build -p mediaway-ffi --target "$t" $profile_flag
done

lib() { echo "$root/target/$1/$profile_dir/libmediaway_ffi.a"; }

cp crates/mediaway-ffi/include/mediaway/*.h "$stage/Headers/mediaway/"
cat > "$stage/Headers/module.modulemap" <<'EOF'
module CMediaway {
    header "mediaway/common.h"
    header "mediaway/container.h"
    header "mediaway/device.h"
    header "mediaway/pipeline.h"
    export *
}
EOF

args=()
# Device slice.
args+=(-library "$(lib aarch64-apple-ios)" -headers "$stage/Headers")

# Simulator slice: arm64, merged with x86_64 when asked (a fat library per platform variant).
sim="$(lib aarch64-apple-ios-sim)"
if [[ "${MEDIAWAY_XCF_X86_SIM:-0}" == "1" ]]; then
  sim="$stage/libmediaway_ffi-sim.a"
  lipo -create "$(lib aarch64-apple-ios-sim)" "$(lib x86_64-apple-ios)" -output "$sim"
fi
args+=(-library "$sim" -headers "$stage/Headers")

if [[ "${MEDIAWAY_XCF_MACOS:-0}" == "1" ]]; then
  args+=(-library "$(lib aarch64-apple-darwin)" -headers "$stage/Headers")
fi

xcodebuild -create-xcframework "${args[@]}" -output "$out/Mediaway.xcframework"

(cd "$out" && ditto -c -k --sequesterRsrc --keepParent Mediaway.xcframework Mediaway.xcframework.zip)
shasum -a 256 "$out/Mediaway.xcframework.zip" | awk '{print $1}' > "$out/Mediaway.xcframework.zip.sha256"

echo
echo "built: $out/Mediaway.xcframework"
echo "zip:   $out/Mediaway.xcframework.zip  (sha256 $(cat "$out/Mediaway.xcframework.zip.sha256"))"
rm -rf "$stage"
