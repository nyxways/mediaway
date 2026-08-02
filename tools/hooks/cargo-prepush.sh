#!/usr/bin/env bash
# tools/hooks/cargo-prepush.sh — clippy full + tests
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

step "cargo clippy --all-targets --all-features -D warnings"
cargo clippy --workspace --all-targets --all-features -- -D warnings
ok

step "cargo test --workspace"
if command -v cargo-nextest >/dev/null 2>&1; then
    cargo nextest run --workspace
else
    cargo test --workspace
fi
ok
