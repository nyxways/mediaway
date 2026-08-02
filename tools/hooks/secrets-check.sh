#!/usr/bin/env bash
# Optional gitleaks — skip with warning if not installed
set -euo pipefail

if ! command -v gitleaks >/dev/null 2>&1; then
    echo "⚠️  gitleaks not installed — skipping secret scan (install: scoop/brew/cargo)" >&2
    exit 0
fi

gitleaks protect --staged --no-banner --log-level=warn
