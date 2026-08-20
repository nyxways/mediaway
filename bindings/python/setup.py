"""setuptools entry point — pins the wheel platform tag.

The wheel bundles one platform's prebuilt native lib (mediaway/_native/*),
but setuptools sees no compiled extension modules and would tag the wheel
`py3-none-any`. `bdist_wheel.plat_name` forces a real platform tag so PyPI
refuses to serve the wheel to a non-matching interpreter.

`MEDIAWAY_WHEEL_PLATFORM` (set by tools/scripts/build-python-package.ts,
default win-x64 for local/back-compat builds) picks which tag: the Linux and
macOS tags match the glibc/deployment-target floor decided in ADR-0024 (see
docs/adr/0024-multi-platform-native-binding-distribution.md) — an honest
exact-floor tag (PEP 600 `manylinux_2_39`, Ubuntu 24.04's glibc — not
ubuntu-22.04's `manylinux_2_35` as ADR-0024 first proposed; that floor
turned out to be genuinely too old for this workspace's PipeWire/VA-API
dependencies, not just an untested guess), not a looser
`manylinux_2_28`/`manylinux2014` claim this build does not actually satisfy.
"""

import os

from setuptools import setup

_PLAT_NAMES = {
    "win-x64": "win_amd64",
    "linux-x64": "manylinux_2_39_x86_64",
    "osx-x64": "macosx_11_0_x86_64",
    "osx-arm64": "macosx_11_0_arm64",
}

_platform = os.environ.get("MEDIAWAY_WHEEL_PLATFORM", "win-x64")
_plat_name = _PLAT_NAMES.get(_platform)
if _plat_name is None:
    raise SystemExit(
        f"MEDIAWAY_WHEEL_PLATFORM={_platform!r} is not one of {sorted(_PLAT_NAMES)}"
    )

setup(options={"bdist_wheel": {"plat_name": _plat_name}})
