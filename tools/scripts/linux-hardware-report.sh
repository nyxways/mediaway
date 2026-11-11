#!/usr/bin/env bash
# Run mediaway's Linux hardware tests on a real Linux machine and write one report of what the machine has and
# which tests ran against it. Meant for the open roadmap items "Linux Hardware Verification" and "Linux Capture
# Hardware Verification" (docs/roadmap.md): the VA-API, PipeWire, portal and V4L2 backends were only compiled and
# linted before, and their tests skip, passing, when no device opens (`skip: no VA-API display available`).
# This script makes a skip visible instead of a pass.
#
# Usage:
#   tools/scripts/linux-hardware-report.sh                 # report in target/linux-report/<host>-<date>.md
#   MEDIAWAY_LINUX_PACKAGES="-p mediaway-device" tools/scripts/linux-hardware-report.sh
#
# Needs: cargo-nextest (cargo install cargo-nextest --locked) and the build packages of
# docs/contributing/linux-hardware.md. Every probe below is optional: a missing tool is reported, not fatal.
set -uo pipefail

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "error: this script runs on Linux (it reads /dev/dri, PipeWire and the session bus)" >&2
  exit 1
fi

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"

packages="${MEDIAWAY_LINUX_PACKAGES:--p mediaway-decoder -p mediaway-encoder -p mediaway-device}"
out_dir="$root/target/linux-report"
report="$out_dir/$(hostname)-$(date +%Y%m%d-%H%M%S).md"
log="$out_dir/test-output.log"
mkdir -p "$out_dir"

have() { command -v "$1" >/dev/null 2>&1; }

# Print a command's output as a fenced block, or say the tool is missing.
probe() {
  local title="$1"; shift
  printf '\n### %s\n\n' "$title"
  if ! have "$1"; then
    printf '`%s` not installed.\n' "$1"
    return
  fi
  printf '```\n'
  "$@" 2>&1 | head -60
  printf '```\n'
}

{
  echo "# mediaway Linux hardware report"
  echo
  echo "- Date: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "- Commit: $(git rev-parse --short HEAD 2>/dev/null || echo unknown)$(git diff --quiet 2>/dev/null || echo ' (dirty)')"
  echo "- Rust: $(rustc --version 2>/dev/null || echo missing)"
  echo "- Session: XDG_SESSION_TYPE=${XDG_SESSION_TYPE:-unset} WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-unset} DISPLAY=${DISPLAY:-unset}"

  echo
  echo "## The machine"
  probe "Kernel and distribution" sh -c 'uname -srm; grep -E "^(PRETTY_NAME|VERSION_ID)=" /etc/os-release'
  probe "GPU" sh -c 'lspci -nnk | grep -A3 -E "VGA|3D|Display"'
  echo
  echo "### Render and capture nodes"
  echo
  echo '```'
  ls -l /dev/dri/ /dev/video* 2>&1 | head -30
  echo '```'
  echo
  echo "The user is in groups: $(id -nG)"

  echo
  echo "## Video acceleration"
  probe "VA-API (vainfo)" vainfo
  probe "Vulkan (vulkaninfo --summary)" vulkaninfo --summary
  probe "Vulkan Video extensions" sh -c 'vulkaninfo 2>/dev/null | grep -E "VK_KHR_video_(queue|decode|encode)" | sort -u'

  echo
  echo "## Capture"
  probe "Cameras (V4L2)" v4l2-ctl --list-devices
  probe "PipeWire" pw-cli info 0
  probe "PipeWire nodes (microphones, screens)" wpctl status
  probe "XDG desktop portal on the session bus" sh -c 'busctl --user list 2>/dev/null | grep -E "portal|screencast"'
} > "$report"

echo "== cargo nextest ($packages), output in $log"
# --no-capture so the `skip:` lines the tests print reach the log; one thread, the devices are shared.
# shellcheck disable=SC2086
cargo nextest run $packages --no-fail-fast --no-capture -j 1 >"$log" 2>&1
status=$?

skips="$(grep -ci 'skip:' "$log" || true)"
{
  echo
  echo "## Tests"
  echo
  echo "- Exit status: $status"
  echo "- Summary: $(grep -E '^\s*Summary' "$log" | tail -1 | sed 's/^ *//')"
  echo "- Tests that skipped because no device opened: **$skips**"
  echo
  echo "A skipped test passed without touching hardware: it verifies nothing. The roadmap items stay open"
  echo "until the skips for the backend being verified are zero on a machine that has the device."
  echo
  echo "### Skips"
  echo
  echo '```'
  grep -i 'skip:' "$log" | sort | uniq -c | sort -rn | head -40
  echo '```'
  echo
  echo "### Failures"
  echo
  echo '```'
  grep -E '^\s+(FAIL|SIGSEGV|SIGABRT|TIMEOUT)' "$log" | sort -u | head -40
  echo '```'
} >> "$report"

echo "report: $report"
echo "tests exited $status; $skips skipped for want of a device"
exit "$status"
