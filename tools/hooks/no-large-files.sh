#!/usr/bin/env bash
# Block staged files larger than 1MB
set -euo pipefail

LIMIT_BYTES=$((1024 * 1024))
EXIT=0

while IFS= read -r file; do
    [[ -z "$file" ]] && continue
    [[ ! -f "$file" ]] && continue
    SIZE=$(wc -c <"$file" | tr -d ' ')
    if [[ "$SIZE" -gt "$LIMIT_BYTES" ]]; then
        echo "❌ Large file ($SIZE bytes): $file" >&2
        echo "   Limit: ${LIMIT_BYTES} bytes (1MB)" >&2
        EXIT=1
    fi
done < <(git diff --cached --name-only --diff-filter=AM)

exit $EXIT
