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
