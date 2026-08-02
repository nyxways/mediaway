#!/usr/bin/env bash
# tools/hooks/cargo-precommit.sh — sequential fmt + clippy (Windows cargo lock safety)
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
    printf '\033[0;36m[pre-commit]\033[0m %-35s' "$1"
}
ok() {
    local elapsed=$(( $(_ms) - _step_start ))
    echo "✓ ${elapsed}ms"
}

# 1. fmt (apply + restage changed staged .rs)
step "cargo fmt (auto-apply)"
_staged_before=$(git diff --cached --name-only --diff-filter=ACM -- '*.rs' || true)
cargo fmt
if [[ -n "${_staged_before}" ]]; then
    while IFS= read -r f; do
        [[ -z "$f" ]] && continue
        if ! git diff --quiet -- "$f"; then
            git add -- "$f"
        fi
    done <<< "$_staged_before"
fi
ok

# 2. clippy --fix + strict verify (workspace; slim — no crate scoping yet)
step "cargo clippy --fix + -D warnings"
_staged_rs=$(git diff --cached --name-only --diff-filter=ACM -- '*.rs' || true)
if [[ -z "${_staged_rs}" ]]; then
    echo "skip (no staged .rs)"
else
    cargo clippy --workspace --all-targets --fix --allow-dirty --allow-staged
    _staged_after=$(git diff --cached --name-only --diff-filter=ACM -- '*.rs' || true)
    while IFS= read -r f; do
        [[ -z "$f" ]] && continue
        if ! git diff --quiet -- "$f"; then
            git add -- "$f"
        fi
    done <<< "${_staged_after}"
    cargo clippy --workspace --all-targets -- -D warnings
    ok
fi
