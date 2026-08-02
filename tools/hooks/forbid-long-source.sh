#!/usr/bin/env bash
# Block staged source files longer than MAX_LINES (default 1000).
# Split modules instead of growing mega-files.
set -euo pipefail

MAX_LINES="${MEDIAWAY_MAX_SOURCE_LINES:-1000}"
EXIT=0

# Source-like extensions (expand when new languages land in-tree).
SOURCE_RE='\.(rs|c|h|cc|cpp|hpp|ts|tsx|js|jsx|go|zig)$'

while IFS= read -r file; do
    [[ -z "$file" ]] && continue
    [[ ! -f "$file" ]] && continue
    # Skip gitignored local scratch and vendored third-party headers
    case "$file" in
        local/* | */vendor/*) continue ;;
    esac

    # Prefer counting without depending on locale; strip spaces from wc
    lines=$(wc -l <"$file" | tr -d '[:space:]')
    if [[ "$lines" =~ ^[0-9]+$ ]] && [[ "$lines" -gt "$MAX_LINES" ]]; then
        echo "❌ Source file too long ($lines lines): $file" >&2
        echo "   Limit: ${MAX_LINES} lines. Split into modules before commit." >&2
        echo "   Override only with explicit user approval + [skip-hooks: …] (discouraged)." >&2
        EXIT=1
    fi
done < <(git diff --cached --name-only --diff-filter=AM | grep -E "$SOURCE_RE" || true)

exit $EXIT
