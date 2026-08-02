#!/usr/bin/env bash
# Claude Code PreToolUse — block secrets / forbidden Rust patterns on Edit|Write
set -euo pipefail

INPUT=$(cat)
FILE=$(echo "$INPUT" | jq -r '.tool_input.file_path // empty' 2>/dev/null || echo "")
CONTENT=$(echo "$INPUT" | jq -r '.tool_input.content // .tool_input.new_string // empty' 2>/dev/null || echo "")

if [[ -z "$FILE" ]]; then
    exit 0
fi

# If jq missing, allow (don't brick agent on Windows without jq)
if ! command -v jq >/dev/null 2>&1; then
    exit 0
fi

SECRET_PATTERNS=(
    'BEGIN RSA PRIVATE KEY'
    'BEGIN OPENSSH PRIVATE KEY'
    'BEGIN PRIVATE KEY'
    'AKIA[0-9A-Z]{16}'
    'ghp_[A-Za-z0-9]{36}'
    'sk-[A-Za-z0-9]{40,}'
)

for pattern in "${SECRET_PATTERNS[@]}"; do
    if echo "$CONTENT" | grep -qE "$pattern"; then
        echo "❌ Secret pattern detected: $pattern" >&2
        echo "   File: $FILE" >&2
        exit 2
    fi
done

if [[ "$FILE" == *.rs ]]; then
    # Allow in #[cfg(test)] modules roughly by skipping *_tests.rs and tests/
    # Normalize path: convert backslashes (Windows) to forward slashes for pattern matching
    NORMALIZED_FILE="${FILE//\\//}"
    case "$NORMALIZED_FILE" in
        */tests/*|*_tests.rs) exit 0 ;;
    esac
    FORBIDDEN=(
        '\.unwrap\(\)'
        'panic!\('
        'dbg!\('
        'todo!\('
        'unimplemented!\('
    )
    for pattern in "${FORBIDDEN[@]}"; do
        if echo "$CONTENT" | grep -qE "$pattern"; then
            echo "❌ Forbidden pattern in production Rust: $pattern" >&2
            echo "   File: $FILE — see CLAUDE.md absolute rules" >&2
            exit 2
        fi
    done
fi

exit 0
