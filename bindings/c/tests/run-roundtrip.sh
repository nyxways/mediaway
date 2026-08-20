#!/usr/bin/env bash
# run-roundtrip.sh — compile and run the container mux+demux round-trip example
# against the release-built native lib. This is the RC-stage binding check for
# the C ABI: it proves the published lib exports every symbol the header
# promises, by linking against it and running the real example end to end.
#
# Usage:
#   bindings/c/tests/run-roundtrip.sh
#   MEDIAWAY_NATIVE_DIR=/path/to/native bindings/c/tests/run-roundtrip.sh
#
# MEDIAWAY_NATIVE_DIR (default: bindings/native/runtime/<rid for this host>)
# must contain the release artifact — mediaway_ffi.dll + its MinGW import lib
# libmediaway_ffi.dll.a on Windows, or libmediaway_ffi.so/.dylib directly on
# Linux/macOS (ADR-0024). The script compiles examples/container/
# mux_roundtrip.c against <mediaway/container.h>, links it to the lib, then
# runs the resulting binary with the lib's directory on the platform's
# runtime search path so it resolves at load time. Any compile or run
# failure — or a round-trip mismatch reported by the example itself — exits
# nonzero.
#
# Runs on windows-latest with MinGW gcc, or ubuntu-22.04/macos-14 with the
# system gcc/clang; pure CPU, no hardware required.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$repo_root"

case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*) host_os="windows" ;;
    Darwin) host_os="macos" ;;
    *) host_os="linux" ;;
esac

case "$host_os" in
    windows) default_rid="win-x64"; lib_file="mediaway_ffi.dll" ;;
    macos) default_rid="osx-$([ "$(uname -m)" = "arm64" ] && echo arm64 || echo x64)"; lib_file="libmediaway_ffi.dylib" ;;
    linux) default_rid="linux-x64"; lib_file="libmediaway_ffi.so" ;;
esac

native_dir="${MEDIAWAY_NATIVE_DIR:-$repo_root/bindings/native/runtime/$default_rid}"

if [ ! -f "$native_dir/$lib_file" ]; then
    echo "run-roundtrip: $lib_file not found in $native_dir" >&2
    echo "run-roundtrip: stage the release-built native lib there, or set MEDIAWAY_NATIVE_DIR" >&2
    exit 1
fi

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

echo "run-roundtrip: compiling mux_roundtrip.c against $native_dir"
gcc -Icrates/mediaway-ffi/include \
    bindings/c/examples/container/mux_roundtrip.c \
    -L"$native_dir" -lmediaway_ffi \
    -o "$tmpdir/roundtrip"

echo "run-roundtrip: running roundtrip"
case "$host_os" in
    windows) PATH="$native_dir:$PATH" "$tmpdir/roundtrip" ;;
    macos) DYLD_LIBRARY_PATH="$native_dir:${DYLD_LIBRARY_PATH:-}" "$tmpdir/roundtrip" ;;
    linux) LD_LIBRARY_PATH="$native_dir:${LD_LIBRARY_PATH:-}" "$tmpdir/roundtrip" ;;
esac

echo "run-roundtrip: OK"
