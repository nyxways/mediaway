#!/usr/bin/env bash
# Conventional Commits 1.0.0 format only — docs/conventions/commits.md
# Language (English commits/PRs) is policy in AGENTS.md — not enforced here.
set -euo pipefail

MSG_FILE="${1:?usage: commit-msg-check.sh <msg-file>}"
FIRST_LINE=$(head -n1 "$MSG_FILE")

if [[ "$FIRST_LINE" =~ ^(Merge|Revert|fixup!|squash!|amend!) ]]; then
    exit 0
fi

PATTERN='^(feat|fix|docs|refactor|perf|test|build|ci|chore|revert)(\([a-z0-9/_-]+\))?!?: .+'

if ! [[ "$FIRST_LINE" =~ $PATTERN ]]; then
    echo "❌ Commit message format violation:" >&2
    echo "   $FIRST_LINE" >&2
    echo "" >&2
    echo "   Format: <type>(<scope>): <subject>" >&2
    echo "   type: feat|fix|docs|refactor|perf|test|build|ci|chore|revert" >&2
    echo "   Example: feat(encoder): add WMF H.264 backend" >&2
    exit 1
fi

SUBJECT="${FIRST_LINE#*: }"
if [[ ${#SUBJECT} -gt 72 ]]; then
    echo "⚠️  Subject exceeds 72 chars (prefer ≤50): ${#SUBJECT}" >&2
fi

if [[ "$FIRST_LINE" =~ \.$ ]]; then
    echo "❌ Subject must not end with a period" >&2
    exit 1
fi

exit 0
