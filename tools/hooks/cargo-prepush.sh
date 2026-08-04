#!/usr/bin/env bash
# tools/hooks/cargo-prepush.sh — clippy + tests on the affected crate set.
#
# Affected = dependency-tree reachability of the pushed diff (bun
# tools/scripts/ci-affected.ts, the same analysis ci.yml uses): NONE skips
# Rust checks entirely, a set runs clippy/tests on those crates only, ALL
# runs the full workspace. The test-media fixture cache is cleared first so
# stale BLAKE3 constants cannot hide behind a locally cached fixture — a
# stale constant breaks CI, not the push, otherwise.
#
# POSIX-only (no arrays): the scoop-shimmed `bash` on Windows is BusyBox.
set -euo pipefail

_ms() {
    if date +%3N &>/dev/null 2>&1; then
        echo "$(date +%s%3N)"
    else
        echo "$(( $(date +%s) * 1000 ))"
    fi
}

_step_start=0
step() {
    _step_start=$(_ms)
    printf '\033[0;36m[pre-push]\033[0m %-40s' "$1"
}
ok() {
    local elapsed=$(( $(_ms) - _step_start ))
    echo "✓ ${elapsed}ms"
}

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"

AFFECTED="ALL"
if command -v bun >/dev/null 2>&1 && git rev-parse --verify -q origin/main >/dev/null; then
    if ! AFFECTED="$(bun tools/scripts/ci-affected.ts --base origin/main)"; then
        AFFECTED="ALL"
    fi
fi
echo "[pre-push] affected: $AFFECTED"

if [ "$AFFECTED" = "NONE" ]; then
    echo "[pre-push] no Rust code changed — clippy/tests skipped"
    exit 0
fi

PKGS=""
for p in $AFFECTED; do PKGS="$PKGS -p $p"; done

if [ "$AFFECTED" = "ALL" ]; then
    step "cargo clippy --all-targets --all-features -D warnings (workspace)"
    cargo clippy --workspace --all-targets --all-features -- -D warnings
else
    step "cargo clippy --all-targets --all-features -D warnings ($AFFECTED)"
    # shellcheck disable=SC2086
    cargo clippy --all-targets --all-features $PKGS -- -D warnings
fi
ok

# Regenerate test-media fixtures from scratch: a stale cached fixture whose
# BLAKE3 still matches an outdated constant would pass locally and fail CI.
rm -rf "$root/local/.cache/test-media"

if [ "$AFFECTED" = "ALL" ]; then
    step "cargo test (workspace)"
    if command -v cargo-nextest >/dev/null 2>&1; then
        cargo nextest run --workspace --all-features
    else
        cargo test --workspace --all-features
    fi
else
    step "cargo test ($AFFECTED)"
    if command -v cargo-nextest >/dev/null 2>&1; then
        # shellcheck disable=SC2086
        cargo nextest run --all-features $PKGS
    else
        # shellcheck disable=SC2086
        cargo test --all-features $PKGS
    fi
fi
ok

# Cross-cfg smoke (best effort): the dev machine is Windows-only, so a
# cfg-gated break on non-Windows (e.g. a windows-only import in an example)
# passes every local check and fails CI's ubuntu job — that exact class bit
# us on the opus example. A wasm32-target check compiles the same
# not-windows cfg paths with no C deps. Only crates proven wasm32-clean are
# checked (CI's wasm job proves iso-bmff-wasm/encoder/decoder/device; the
# mediaway facade was verified manually); other crates are left to CI.
if rustup target list --installed 2>/dev/null | grep -q '^wasm32-unknown-unknown$'; then
    WASM_SAFE="iso-bmff-wasm mediaway-encoder mediaway-decoder mediaway-device mediaway"
    WASM_PKGS=""
    for p in $AFFECTED; do
        case " $WASM_SAFE " in
            *" $p "*) WASM_PKGS="$WASM_PKGS -p $p" ;;
        esac
    done
    if [ -n "$WASM_PKGS" ]; then
        step "cargo check (wasm32 cross-cfg$WASM_PKGS)"
        # lib/bins/examples only: benches pull criterion→Rayon, which refuses
        # wasm32, and tests need a runner we don't have for this target.
        if cargo check --target wasm32-unknown-unknown $WASM_PKGS --lib --bins --examples --all-features >/tmp/mw-wasm.log 2>&1; then
            ok
        else
            echo "✗ wasm32 cross-cfg check failed:"
            grep -E "^error" -A6 /tmp/mw-wasm.log | head -24
            exit 1
        fi
    fi
fi
