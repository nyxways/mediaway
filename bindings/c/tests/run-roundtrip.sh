#!/usr/bin/env bash
# run-roundtrip.sh — compile and run the container mux+demux round-trip example
# against the release-built native DLL. This is the RC-stage binding check for
# the C ABI: it proves the published mediaway_ffi.dll exports every symbol the
# header promises, by linking against it and running the real example end to end.
#
# Usage:
#   bindings/c/tests/run-roundtrip.sh
#   MEDIAWAY_NATIVE_DIR=/path/to/native bindings/c/tests/run-roundtrip.sh
#
# MEDIAWAY_NATIVE_DIR (default: bindings/native/runtime/win-x64) must contain the
# release artifact mediaway_ffi.dll plus its MinGW import lib libmediaway_ffi.dll.a.
# The script compiles examples/container/mux_roundtrip.c against
# <mediaway/container.h>, links it to the DLL, then runs the resulting exe with
# the DLL's directory on PATH so Windows resolves it at load time. Any compile or
# run failure — or a round-trip mismatch reported by the example itself — exits
# nonzero.
#
# Runs on windows-latest with MinGW gcc; pure CPU, no hardware required.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$repo_root"

native_dir="${MEDIAWAY_NATIVE_DIR:-$repo_root/bindings/native/runtime/win-x64}"

if [ ! -f "$native_dir/mediaway_ffi.dll" ]; then
    echo "run-roundtrip: mediaway_ffi.dll not found in $native_dir" >&2
    echo "run-roundtrip: stage the release-built native DLL there, or set MEDIAWAY_NATIVE_DIR" >&2
    exit 1
fi

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

echo "run-roundtrip: compiling mux_roundtrip.c against $native_dir"
gcc -Icrates/mediaway-ffi/include \
    bindings/c/examples/container/mux_roundtrip.c \
    -L"$native_dir" -lmediaway_ffi \
    -o "$tmpdir/roundtrip.exe"

echo "run-roundtrip: running roundtrip.exe"
PATH="$native_dir:$PATH" "$tmpdir/roundtrip.exe"

echo "run-roundtrip: OK"
