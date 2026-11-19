"""ctypes bindings over Mediaway's C ABI (mediaway-*-ffi).

This module is the raw ABI layer: struct layouts and function prototypes,
mirroring `crates/mediaway-*-ffi/include/mediaway/*.h` exactly. Nothing here
is idiomatic — the wrappers in `mediaway/_container.py`, `_encoder.py`, and
`_device.py` translate this into Python.

It is one namespace over three files, split to stay under the 1000-line source
convention: `_ffi_base` (library discovery, shared value types, container.h),
`_ffi_pipeline` (pipeline.h) and `_ffi_device` (device.h). The replay ring lives in
`_ffi_replay`. Wrappers keep using `_ffi.<name>` for everything.

Ownership rules (from the headers):
  - Borrowed inputs (track extra_data, packet payload, push_bytes data, frame
    raw_bytes, decryption key) are caller-owned, valid for the call only. The
    wrappers copy in/out so Python callers never hold native memory.
  - Owned outputs (poll_bytes buffers, demuxed packets/stream info, finish
    buffers, polled device frames) MUST be released through the matching
    `_free` function — the wrappers do this automatically.
"""

from __future__ import annotations

from ._ffi_base import *  # noqa: F401,F403
from ._ffi_device import *  # noqa: F401,F403
from ._ffi_pipeline import *  # noqa: F401,F403

__all__ = [
    "container",
    "pipeline",
    "device",
    "lib_dir",
]
