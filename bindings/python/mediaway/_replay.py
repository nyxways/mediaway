"""Replay ring: keep the last N seconds of encoded packets, cut a clip at a keyframe.

Wraps the container C ABI's replay ring (`adr/container/0009-replay-ring-c-abi.md`).
Sans-io like the rest of the container capability: no clock, no files.

LIMIT: the C ABI has no video-packet source. `EncodeSession` muxes its
encoder's packets internally, and packet-level output exists only for the audio
encoder. Feed the ring packets from a `Demuxer`, from an `AudioEncoder`, or from
an encoder you drive yourself.

A ring keeps one anchor stream (normally video) whose keyframes decide where a
clip may start; other streams (audio) are cut by time to match. Eviction and cuts
use decode order, whole GOPs at a time, so a clip can start up to one keyframe
interval EARLIER than asked, never later. To write a clip, push its packets
through a `Muxer` (see `examples/container/replay_ring.py`); there is
deliberately no one-shot "save this clip" call, because the ring does not hold
each track's codec configuration.

Timestamps are `Rational` seconds, as everywhere in this package; stream ids and
time bases are the ones you gave `ReplayRing`/`add_stream`, and they must match
the muxer's track ids (the first `Muxer.add_*_track` returns 1).
"""

from __future__ import annotations

from ctypes import byref, c_bool, c_size_t, c_uint64, c_void_p, cast, create_string_buffer
from datetime import timedelta
from typing import Iterator

from . import _ffi, _ffi_replay
from ._container import _check_container, _copy_bytes, _from_units, _to_units
from ._errors import InvalidStateError, MediawayError, OutOfOrderPacketError, UnknownStreamError
from ._types import Packet, PacketMeta, Rational, ReplayClipEntry, StoredPayload

__all__ = ["ReplayRing", "ReplayClip"]

_PAYLOAD_KINDS = {"bytes": _ffi_replay.REPLAY_PAYLOAD_BYTES, "stored": _ffi_replay.REPLAY_PAYLOAD_STORED}


def _check_replay(status: int) -> None:
    """Like the container check, but the ring's own outcomes get distinct exceptions."""
    if status == _ffi.MEDIAWAY_STATUS_UNKNOWN_STREAM:
        raise UnknownStreamError(status, "packet's stream was never added to the replay ring")
    if status == _ffi.MEDIAWAY_STATUS_INVALID_PACKET:
        raise OutOfOrderPacketError(
            status, "packet's dts went backwards in its stream; it was not added, carry on"
        )
    _check_container(status)


def _millis(value: timedelta | float) -> int:
    """A `timedelta` or a number of seconds, as whole milliseconds."""
    seconds = value.total_seconds() if isinstance(value, timedelta) else float(value)
    if seconds < 0:
        raise ValueError("a replay duration cannot be negative")
    return round(seconds * 1000)


def _ticks(seconds: Rational | None, time_base: Rational) -> int:
    return 0 if seconds is None else _to_units(seconds, time_base)


class ReplayRing:
    """A rolling buffer of the last `window` of encoded packets.

    `anchor_stream_id` names the stream whose keyframes decide where a clip may
    start; `anchor_time_base` is its tick, like `VideoStreamInfo.frame_rate`
    (`Rational(1, 30)` = 30 fps). `window` is a `timedelta` or seconds.
    `max_bytes` (0 = no ceiling) evicts the oldest GOP first past that many
    payload bytes; the newest GOP is always kept.

    `payload="bytes"` holds each pushed payload (`push`); `payload="stored"`
    holds only where it is (`push_stored`), for a caller that already writes
    packets to a file. The other kind's push raises `InvalidStateError`.
    """

    def __init__(
        self,
        anchor_stream_id: int,
        anchor_time_base: Rational,
        window: timedelta | float,
        max_bytes: int = 0,
        payload: str = "bytes",
    ):
        try:
            kind = _PAYLOAD_KINDS[payload]
        except KeyError:
            raise ValueError(f"payload must be 'bytes' or 'stored', not {payload!r}") from None
        if max_bytes < 0:
            raise ValueError("max_bytes cannot be negative")
        config = _ffi_replay.ReplayRingConfig(
            anchor_stream_id=anchor_stream_id,
            anchor_time_base=_ffi.Rational(anchor_time_base.num, anchor_time_base.den),
            window_ms=_millis(window),
            max_bytes=max_bytes,
            payload_kind=kind,
        )
        handle = c_void_p()
        _check_replay(_ffi.container.dll.mediaway_replay_ring_create(byref(config), byref(handle)))
        self._handle = handle.value
        self._payload = payload
        self._time_bases: dict[int, Rational] = {anchor_stream_id: anchor_time_base}

    def _require_open(self) -> int:
        if not self._handle:
            raise InvalidStateError(_ffi.MEDIAWAY_STATUS_INVALID_STATE, "the replay ring is closed")
        return self._handle

    def add_stream(self, stream_id: int, time_base: Rational) -> None:
        """Carry another stream (e.g. audio), cut by time to match the anchor."""
        handle = self._require_open()
        raw = _ffi.Rational(time_base.num, time_base.den)
        _check_replay(_ffi_replay._H.mediaway_replay_ring_add_stream(handle, stream_id, raw))
        self._time_bases[stream_id] = time_base

    def _time_base(self, stream_id: int) -> Rational:
        try:
            return self._time_bases[stream_id]
        except KeyError:
            raise UnknownStreamError(
                _ffi.MEDIAWAY_STATUS_UNKNOWN_STREAM,
                f"stream {stream_id} was never added to the replay ring",
            ) from None

    def push(self, packet: Packet) -> None:
        """Add a packet to a `payload="bytes"` ring, then evict what fell out of the window.

        The payload is COPIED into the ring (the native side borrows it for the
        call only), and every clip then shares that copy. Anchor packets before
        the anchor's first keyframe are dropped without error: nothing can start
        from them.

        Raises `UnknownStreamError` for a stream never added, and
        `OutOfOrderPacketError` when the stream's `dts` went backwards (the packet
        was not added; carry on).
        """
        handle = self._require_open()
        tb = self._time_base(packet.stream_index)
        dts = packet.dts if packet.dts is not None else packet.pts
        raw = _ffi.PacketView(
            stream_id=packet.stream_index,
            pts=_to_units(packet.pts, tb),
            dts=_to_units(dts, tb),
            duration=_ticks(packet.duration, tb),
            is_keyframe=packet.key,
            is_discard=False,
            payload=None,
            payload_len=0,
        )
        if packet.payload:
            buf = create_string_buffer(packet.payload, len(packet.payload))
            raw.payload = cast(buf, _ffi.U8P)
            raw.payload_len = len(packet.payload)
        _check_replay(_ffi_replay._H.mediaway_replay_ring_push(handle, byref(raw)))

    def push_stored(self, meta: PacketMeta, stored: StoredPayload) -> None:
        """Add a packet to a `payload="stored"` ring: its metadata, and where its bytes are.

        The ring never reads the file and holds a few dozen bytes per packet. Take
        `stored` from `LiveMuxer.poll_placements()` when you write every polled
        muxer byte to a file. Same errors as `push`.
        """
        handle = self._require_open()
        tb = self._time_base(meta.stream_index)
        dts = meta.dts if meta.dts is not None else meta.pts
        raw_meta = _ffi_replay.PacketMeta(
            stream_id=meta.stream_index,
            pts=_to_units(meta.pts, tb),
            dts=_to_units(dts, tb),
            duration=_ticks(meta.duration, tb),
            is_keyframe=meta.key,
            is_discard=meta.discard,
        )
        raw_stored = _ffi_replay.StoredPayload(file=stored.file, offset=stored.offset, len=stored.length)
        _check_replay(
            _ffi_replay._H.mediaway_replay_ring_push_stored(handle, byref(raw_meta), byref(raw_stored))
        )

    def span(self) -> timedelta:
        """The longest clip `clip_last` can return right now; zero before the anchor's first keyframe."""
        handle = self._require_open()
        ms = c_uint64(0)
        _check_replay(_ffi_replay._H.mediaway_replay_ring_span_ms(handle, byref(ms)))
        return timedelta(milliseconds=ms.value)

    def clip_last(self, span: timedelta | float) -> "ReplayClip | None":
        """Cut the last `span` of every stream at a keyframe.

        The clip starts at the latest anchor keyframe at or before `newest - span`:
        up to one keyframe interval EARLIER than asked, never later; with less than
        `span` held it starts at the oldest keyframe. Returns None until the
        anchor's first keyframe has been pushed.

        The clip is an OWNED SNAPSHOT: keep pushing while you read it, and it stays
        valid even after this ring is closed.
        """
        handle = self._require_open()
        clip = c_void_p()
        has = c_bool(False)
        _check_replay(
            _ffi_replay._H.mediaway_replay_ring_clip_last(handle, _millis(span), byref(clip), byref(has))
        )
        if not has.value:
            return None
        return ReplayClip(clip.value, dict(self._time_bases), self._payload)

    def __enter__(self) -> "ReplayRing":
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.close()

    def close(self) -> None:
        """Free the ring. Safe to call twice. Clips already taken stay valid."""
        if self._handle:
            _ffi_replay._H.mediaway_replay_ring_close(self._handle)
            self._handle = None


class ReplayClip:
    """A cut of a `ReplayRing`: which packets, with timestamps rebased to zero.

    An OWNED SNAPSHOT — unlike the Rust API's borrowing `Clip`, it stays valid
    after further `push` calls (which may evict what it was cut from) and after
    the ring is closed. `len(clip)` is the packet count; `clip[i]` and iteration
    give `ReplayClipEntry`s in decode order across streams.

    For a Bytes ring an entry's `payload` is a COPY of the clip's bytes, made
    when the entry is read (the native pointer is only borrowed from the clip).
    Use it as a context manager, or call `close()`, to free the snapshot.
    """

    def __init__(self, handle: int, time_bases: dict[int, Rational], payload: str):
        self._handle = handle
        self._time_bases = time_bases
        self._payload = payload

    def _require_open(self) -> int:
        if not self._handle:
            raise InvalidStateError(_ffi.MEDIAWAY_STATUS_INVALID_STATE, "the replay clip is closed")
        return self._handle

    def __len__(self) -> int:
        return _ffi_replay._H.mediaway_replay_clip_packet_count(self._require_open())

    @property
    def duration(self) -> timedelta:
        """From the cut keyframe to the newest packet pushed when the clip was taken."""
        ms = c_uint64(0)
        _check_replay(_ffi_replay._H.mediaway_replay_clip_duration_ms(self._require_open(), byref(ms)))
        return timedelta(milliseconds=ms.value)

    def __getitem__(self, index: int) -> ReplayClipEntry:
        handle = self._require_open()
        count = len(self)
        if not isinstance(index, int):
            raise TypeError("clip indices must be integers")
        if index < 0:
            index += count
        if not 0 <= index < count:
            raise IndexError("replay clip index out of range")
        raw = _ffi_replay.ReplayClipEntry()
        _check_replay(_ffi_replay._H.mediaway_replay_clip_packet_at(handle, c_size_t(index), byref(raw)))
        tb = self._time_bases.get(raw.stream_id)
        if tb is None:
            raise MediawayError(_ffi.MEDIAWAY_STATUS_UNKNOWN_STREAM, f"clip has unknown stream {raw.stream_id}")
        if raw.payload_kind == _ffi_replay.REPLAY_PAYLOAD_BYTES:
            payload = _copy_bytes(raw.payload, raw.payload_len)
            stored = None
        else:
            payload = None
            stored = StoredPayload(file=raw.stored_file, offset=raw.stored_offset, length=raw.stored_len)
        return ReplayClipEntry(
            stream_index=raw.stream_id,
            pts=_from_units(raw.pts, tb),
            dts=_from_units(raw.dts, tb),
            key=raw.is_keyframe,
            discard=raw.is_discard,
            duration=_from_units(raw.duration, tb) if raw.duration else None,
            payload=payload,
            stored=stored,
        )

    def __iter__(self) -> Iterator[ReplayClipEntry]:
        for index in range(len(self)):
            yield self[index]

    def __enter__(self) -> "ReplayClip":
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.close()

    def close(self) -> None:
        """Free the snapshot. Safe to call twice."""
        if self._handle:
            _ffi_replay._H.mediaway_replay_clip_free(self._handle)
            self._handle = None
