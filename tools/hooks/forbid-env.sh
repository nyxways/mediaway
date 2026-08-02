#!/usr/bin/env bash
# Block secret-grade files from staging
set -euo pipefail

PATTERNS=(
    '\.env(\.|$)'
    'secrets\.env'
    '\.pem$'
    'id_rsa($|\.)'
    'id_ed25519($|\.)'
    'credentials.*\.json'
    'private.*\.key'
)

EXIT=0

while IFS= read -r file; do
    [[ -z "$file" ]] && continue
    for pattern in "${PATTERNS[@]}"; do
        if [[ "$file" =~ $pattern ]]; then
            echo "❌ Secret-grade file staged: $file" >&2
            echo "   Pattern: $pattern" >&2
            echo "   See docs/conventions/security.md" >&2
            EXIT=1
        fi
    done
done < <(git diff --cached --name-only --diff-filter=AM)

exit $EXIT
