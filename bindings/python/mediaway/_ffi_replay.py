"""ctypes bindings for the container C ABI's replay ring and MP4 payload
placements (`adr/container/0009-replay-ring-c-abi.md`).

Split out of `_ffi.py`, which is already past the workspace's 1000-line source
convention, rather than growing it further. Struct layouts mirror the tail of
`crates/mediaway-ffi/include/mediaway/container.h` exactly; the sizes and
offsets are pinned in `tests/test_replay_ring.py` against a gcc probe of the
real header.

Ownership (from the header):
  - `push` copies the borrowed payload in; nothing here holds Python memory.
  - A clip is an OWNED SNAPSHOT; a Bytes entry's `payload` pointer is borrowed
    from the clip and valid until `mediaway_replay_clip_free`. The wrappers copy
    it out immediately.
  - `mediaway_muxer_poll_placements` returns an owned array that MUST be
    released with `mediaway_placements_free`.
"""

from __future__ import annotations

from ctypes import (
    POINTER,
    Structure,
    c_bool,
    c_int32,
    c_int64,
    c_size_t,
    c_uint32,
    c_uint64,
    c_void_p,
)

from . import _ffi
from ._ffi import U8P, Rational

__all__ = [
    "CONTAINER_ABI_VERSION",
    "REPLAY_PAYLOAD_BYTES",
    "REPLAY_PAYLOAD_STORED",
    "ReplayRingConfig",
    "PacketMeta",
    "StoredPayload",
    "ReplayClipEntry",
    "Placement",
]

#: The container ABI version this module's layouts were written against.
CONTAINER_ABI_VERSION = 8

# enum mediaway_replay_payload_kind
REPLAY_PAYLOAD_BYTES = 0  # payload copied in by mediaway_replay_ring_push
REPLAY_PAYLOAD_STORED = 1  # only where the bytes are (mediaway_replay_ring_push_stored)


class ReplayRingConfig(Structure):  # plain value; no free
    _fields_ = [
        ("anchor_stream_id", c_uint32),
        ("anchor_time_base", Rational),
        ("window_ms", c_uint64),
        ("max_bytes", c_uint64),  # 0 = no ceiling
        ("payload_kind", c_int32),
    ]


class PacketMeta(Structure):  # a packet's metadata without its payload
    _fields_ = [
        ("stream_id", c_uint32),
        ("pts", c_int64),
        ("dts", c_int64),
        ("duration", c_uint64),
        ("is_keyframe", c_bool),
        ("is_discard", c_bool),
    ]


class StoredPayload(Structure):  # where a payload was stored on the caller's disk
    _fields_ = [
        ("file", c_uint32),
        ("offset", c_uint64),
        ("len", c_uint32),
    ]


class ReplayClipEntry(Structure):  # one clip packet; payload BORROWED from the clip
    _fields_ = [
        ("stream_id", c_uint32),
        ("pts", c_int64),
        ("dts", c_int64),
        ("duration", c_uint64),
        ("is_keyframe", c_bool),
        ("is_discard", c_bool),
        ("payload_kind", c_int32),
        ("payload", U8P),  # BORROWED; Bytes rings only, else NULL
        ("payload_len", c_size_t),
        ("stored_file", c_uint32),  # Stored rings only
        ("stored_offset", c_uint64),
        ("stored_len", c_uint32),
    ]


class Placement(Structure):  # owned array element from poll_placements
    _fields_ = [
        ("track_id", c_uint32),
        ("dts", c_int64),
        ("offset", c_uint64),
        ("len", c_uint32),
    ]


_H = _ffi.container.dll

_H.mediaway_replay_ring_create.restype = c_int32
_H.mediaway_replay_ring_create.argtypes = [POINTER(ReplayRingConfig), POINTER(c_void_p)]
_H.mediaway_replay_ring_add_stream.restype = c_int32
_H.mediaway_replay_ring_add_stream.argtypes = [c_void_p, c_uint32, Rational]
_H.mediaway_replay_ring_push.restype = c_int32
_H.mediaway_replay_ring_push.argtypes = [c_void_p, POINTER(_ffi.PacketView)]
_H.mediaway_replay_ring_push_stored.restype = c_int32
_H.mediaway_replay_ring_push_stored.argtypes = [c_void_p, POINTER(PacketMeta), POINTER(StoredPayload)]
_H.mediaway_replay_ring_span_ms.restype = c_int32
_H.mediaway_replay_ring_span_ms.argtypes = [c_void_p, POINTER(c_uint64)]
_H.mediaway_replay_ring_clip_last.restype = c_int32
_H.mediaway_replay_ring_clip_last.argtypes = [c_void_p, c_uint64, POINTER(c_void_p), POINTER(c_bool)]
_H.mediaway_replay_ring_close.restype = None
_H.mediaway_replay_ring_close.argtypes = [c_void_p]

_H.mediaway_replay_clip_packet_count.restype = c_size_t
_H.mediaway_replay_clip_packet_count.argtypes = [c_void_p]
_H.mediaway_replay_clip_duration_ms.restype = c_int32
_H.mediaway_replay_clip_duration_ms.argtypes = [c_void_p, POINTER(c_uint64)]
_H.mediaway_replay_clip_packet_at.restype = c_int32
_H.mediaway_replay_clip_packet_at.argtypes = [c_void_p, c_size_t, POINTER(ReplayClipEntry)]
_H.mediaway_replay_clip_free.restype = None
_H.mediaway_replay_clip_free.argtypes = [c_void_p]

_H.mediaway_muxer_create_with_placements.restype = c_void_p
_H.mediaway_muxer_create_with_placements.argtypes = []
_H.mediaway_muxer_poll_placements.restype = c_int32
_H.mediaway_muxer_poll_placements.argtypes = [c_void_p, POINTER(POINTER(Placement)), POINTER(c_size_t)]
_H.mediaway_placements_free.restype = None
_H.mediaway_placements_free.argtypes = [POINTER(Placement), c_size_t]
