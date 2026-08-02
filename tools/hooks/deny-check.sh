#!/usr/bin/env bash
# tools/hooks/deny-check.sh — cargo deny (advisories + licenses + bans + sources)
set -euo pipefail

# Avoid advisory-db fetch using this repo's .git
env -u GIT_DIR -u GIT_WORK_TREE -u GIT_INDEX_FILE \
    cargo deny check advisories licenses bans sources
