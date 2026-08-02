#!/usr/bin/env bash
# Bare TODO/FIXME blocked — use TODO(#issue)
set -euo pipefail

EXIT=0

while IFS= read -r file; do
    [[ -z "$file" ]] && continue
    [[ ! -f "$file" ]] && continue

    while IFS= read -r line; do
        if [[ "$line" =~ (TODO|FIXME) ]]; then
            if ! [[ "$line" =~ (TODO|FIXME)\(#[0-9]+\) ]]; then
                echo "❌ Bare TODO/FIXME: $file" >&2
                echo "   $line" >&2
                echo "   Use: TODO(#NNN) or FIXME(#NNN)" >&2
                EXIT=1
            fi
        fi
    done < <(git diff --cached "$file" | grep '^+' | grep -v '^+++' || true)
done < <(git diff --cached --name-only --diff-filter=AM | grep -E '\.(rs|ts|js)$' || true)

exit $EXIT
