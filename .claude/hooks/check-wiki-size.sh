#!/usr/bin/env bash
# PreToolUse: reject docs/ai/wiki markdown over 100 lines.
# If jq is missing, allow quietly (safe fallback).
command -v jq &>/dev/null || exit 0

input=$(cat)
tool_name=$(echo "$input" | jq -r '.tool_name // ""')
file_path=$(echo "$input" | jq -r '.tool_input.file_path // ""')
# normalize backslashes for Windows paths
file_path="${file_path//\\//}"

[[ "$file_path" == *"docs/ai/wiki/"*".md" ]] || exit 0

LIMIT=100

case "$tool_name" in
  Write)
    line_count=$(echo "$input" | jq -r '.tool_input.content // ""' | awk 'END{print NR}')
    ;;
  Edit)
    [[ -f "$file_path" ]] || exit 0
    current=$(awk 'END{print NR}' "$file_path")
    old_lines=$(echo "$input" | jq -r '.tool_input.old_string // ""' | awk 'END{print NR}')
    new_lines=$(echo "$input" | jq -r '.tool_input.new_string // ""' | awk 'END{print NR}')
    line_count=$(( current - old_lines + new_lines ))
    ;;
  *)
    exit 0
    ;;
esac

if (( line_count > LIMIT )); then
  if command -v jq &>/dev/null; then
    jq -n \
      --argjson l "$line_count" \
      --argjson lim "$LIMIT" \
      '{"hookSpecificOutput":{"permissionDecision":"deny"},"systemMessage":"Wiki file is \($l) lines (limit \($lim)). Split into another file and update index.md."}'
  else
    echo "❌ Wiki file is ${line_count} lines (limit ${LIMIT}). Split and update index.md." >&2
    exit 2
  fi
fi
