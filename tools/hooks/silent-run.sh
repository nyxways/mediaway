#!/usr/bin/env bash
# tools/hooks/silent-run.sh — quiet on success, full output on failure
set -euo pipefail

log_dir="${MEDIAWAY_HOOK_LOG_DIR:-${TMPDIR:-/tmp}/mediaway-hook-logs}"
mkdir -p "$log_dir"
slug=$(echo "$*" | tr -c 'A-Za-z0-9' '_' | cut -c1-80)
log="$log_dir/$(date +%Y%m%d-%H%M%S)-${slug}.log"

_now_ms() {
    local t
    if t=$(date +%s%3N 2>/dev/null) && [[ ${#t} -gt 10 ]]; then
        echo "$t"
    else
        echo "$(( $(date +%s) * 1000 ))"
    fi
}
start=$(_now_ms)

if out=$("$@" 2>&1); then
    elapsed=$(( $(_now_ms) - start ))
    printf '%s\n' "$out" >"$log"
    if [[ "${MEDIAWAY_HOOK_VERBOSE:-0}" == "1" ]]; then
        printf '[silent-run] OK (%dms): %s\n' "$elapsed" "$*" >&2
    fi
    exit 0
else
    rc=$?
fi
elapsed=$(( $(_now_ms) - start ))
printf '%s\n' "$out" >"$log"
printf '%s\n' "$out" >&2
printf '\n[silent-run] FAIL (rc=%d, %dms) — full log: %s\n' "$rc" "$elapsed" "$log" >&2
exit "$rc"
