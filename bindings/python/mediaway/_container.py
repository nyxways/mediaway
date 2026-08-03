"""Container capability: sans-io fragmented-MP4 mux + demux.

Wraps the `mediaway-ffi` C ABI (see `_ffi.py` for the raw layer and
`../README.md` for the DX contract). Typestate is two classes: `Muxer` (Open —
tracks can be registered) and `LiveMuxer` (streaming — packets can be pushed),
returned by `Muxer.begin()`. The muxer never touches files or sockets: the
caller owns all byte I/O.

Timestamps are `Rational` seconds; the ABI works in integer units of each
track's time base, so packets are converted on the way in/out.
"""

from __future__ import annotations

import ctypes
import math
from ctypes import byref, c_bool, c_size_t, c_ubyte, cast, create_string_buffer

from . import _ffi
from ._errors import InvalidStateError, MediawayError
from ._types import AudioStreamInfo, Codec, Packet, Rational, VideoStreamInfo

__all__ = ["Muxer", "LiveMuxer", "Demuxer"]


def _to_units(seconds: Rational, time_base: Rational) -> int:
    """Convert Rational seconds to integer ticks of `time_base` (rounded)."""
    # ticks = seconds / (tb.num / tb.den) = seconds.num*tb.den / (seconds.den*tb.num)
    num = seconds.num * time_base.den
    den = seconds.den * time_base.num
    return (num + den // 2) // den


def _from_units(ticks: int, time_base: Rational) -> Rational:
    """Convert integer ticks of `time_base` to Rational seconds (reduced)."""
    num = ticks * time_base.num
    den = time_base.den
    g = math.gcd(num, den)
    return Rational(num // g, den // g)


def _copy_bytes(ptr, length: int) -> bytes:
    if not ptr or length == 0:
        return b""
    return ctypes.string_at(ptr, length)


def _check_container(status: int) -> None:
    if status == _ffi.MEDIAWAY_OK:
        return
    names = {
        _ffi.MEDIAWAY_STATUS_INVALID_ARGUMENT: "invalid argument",
        _ffi.MEDIAWAY_STATUS_INVALID_STATE: "invalid state",
        _ffi.MEDIAWAY_STATUS_INVALID_TRACK: "invalid or duplicate track id",
        _ffi.MEDIAWAY_STATUS_INVALID_PACKET: "packet does not match a registered track",
        _ffi.MEDIAWAY_STATUS_INVALID_DATA: "truncated or malformed container data",
        _ffi.MEDIAWAY_STATUS_UNKNOWN_ERROR: "unknown error",
        _ffi.MEDIAWAY_STATUS_INTERNAL_PANIC: "internal panic (handle poisoned)",
        _ffi.MEDIAWAY_STATUS_HANDLE_POISONED: "handle poisoned by an earlier panic",
    }
    message = names.get(status, "unknown status")
    cls = InvalidStateError if status == _ffi.MEDIAWAY_STATUS_INVALID_STATE else MediawayError
    raise cls(status, message)


class Muxer:
    """A muxer in the track-registration (Open) state.

    Stream ids are assigned in registration order: the first `add_*_track`
    call returns 0, the second 1, and so on. `begin()` consumes this object
    (its handle moves into the returned `LiveMuxer`), making "add_track after
    begin" a Python AttributeError instead of the ABI's INVALID_STATE.
    """

    def __init__(self, fragment_batch: int | None = None):
        dll = _ffi.container.dll
        if fragment_batch is None:
            self._handle = dll.mediaway_muxer_create()
        else:
            self._handle = dll.mediaway_muxer_create_with_fragment_batch(fragment_batch)
        if not self._handle:
            raise MediawayError(_ffi.MEDIAWAY_STATUS_INTERNAL_PANIC, "muxer creation panicked")
        self._next_id = 0
        self._time_bases: dict[int, Rational] = {}  # stream id -> ABI time base

    def _assign_id(self) -> int:
        track_id = self._next_id
        self._next_id += 1
        return track_id

    def add_video_track(self, info: VideoStreamInfo) -> int:
        track_id = self._assign_id()
        raw = _ffi.VideoTrackInfo(
            id=track_id,
            codec=int(info.codec),
            time_base=_ffi.Rational(info.frame_rate.num, info.frame_rate.den),
            width=info.width,
            height=info.height,
            extra_data=None,
            extra_data_len=0,
        )
        if info.extra_data:
            buf = create_string_buffer(info.extra_data, len(info.extra_data))
            raw.extra_data = cast(buf, _ffi.U8P)
            raw.extra_data_len = len(info.extra_data)
        _check_container(_ffi.container.dll.mediaway_muxer_add_video_track(self._handle, byref(raw)))
        self._time_bases[track_id] = info.frame_rate
        return track_id

    def add_audio_track(self, info: AudioStreamInfo) -> int:
        track_id = self._assign_id()
        raw = _ffi.AudioTrackInfo(
            id=track_id,
            codec=int(info.codec),
            time_base=_ffi.Rational(1, info.sample_rate),
            sample_rate=info.sample_rate,
            channels=info.channels,
            extra_data=None,
            extra_data_len=0,
        )
        if info.extra_data:
            buf = create_string_buffer(info.extra_data, len(info.extra_data))
            raw.extra_data = cast(buf, _ffi.U8P)
            raw.extra_data_len = len(info.extra_data)
        _check_container(_ffi.container.dll.mediaway_muxer_add_audio_track(self._handle, byref(raw)))
        self._time_bases[track_id] = Rational(1, info.sample_rate)
        return track_id

    def begin(self) -> "LiveMuxer":
        _check_container(_ffi.container.dll.mediaway_muxer_begin(self._handle))
        live = LiveMuxer(self._handle, self._time_bases)
        self._handle = None  # ownership transferred to the LiveMuxer
        return live

    def __enter__(self) -> "Muxer":
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.close()

    def close(self) -> None:
        if self._handle:
            _ffi.container.dll.mediaway_muxer_close(self._handle)
            self._handle = None


class LiveMuxer:
    """A muxer in the streaming (Live) state, from `Muxer.begin()`."""

    def __init__(self, handle: int, time_bases: dict[int, Rational]):
        self._handle = handle
        self._time_bases = time_bases  # stream id -> ABI time base

    def push_packet(self, packet: Packet) -> None:
        dts = packet.dts if packet.dts is not None else packet.pts
        duration = packet.duration
        try:
            tb = self._time_bases[packet.stream_index]
        except KeyError:
            raise MediawayError(
                _ffi.MEDIAWAY_STATUS_INVALID_TRACK,
                f"no track with stream_index {packet.stream_index}",
            ) from None
        raw = _ffi.PacketView(
            stream_id=packet.stream_index,
            pts=0,
            dts=0,
            duration=0,
            is_keyframe=packet.key,
            is_discard=False,
            payload=None,
            payload_len=0,
        )
        if packet.payload:
            buf = create_string_buffer(packet.payload, len(packet.payload))
            raw.payload = cast(buf, _ffi.U8P)
            raw.payload_len = len(packet.payload)
        # Convert Rational seconds -> integer ticks of the stream's time base.
        raw.pts = _to_units(packet.pts, tb)
        raw.dts = _to_units(dts, tb)
        if duration is not None:
            raw.duration = _to_units(duration, tb)
        _check_container(_ffi.container.dll.mediaway_muxer_push_packet(self._handle, byref(raw)))

    def flush(self) -> None:
        _check_container(_ffi.container.dll.mediaway_muxer_flush(self._handle))

    def poll_bytes(self) -> bytes | None:
        """Drain whatever fMP4 bytes are ready now; None when nothing is ready."""
        out_data = _ffi.U8P()
        out_len = c_size_t(0)
        _check_container(
            _ffi.container.dll.mediaway_muxer_poll_bytes(self._handle, byref(out_data), byref(out_len))
        )
        length = out_len.value
        if length == 0:
            return None
        data = _copy_bytes(out_data, length)
        _ffi.container.dll.mediaway_buffer_free(out_data, out_len)
        return data

    def __enter__(self) -> "LiveMuxer":
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.close()

    def close(self) -> None:
        if self._handle:
            _ffi.container.dll.mediaway_muxer_close(self._handle)
            self._handle = None


class Demuxer:
    """A streaming demuxer: feed container bytes, poll streams and packets."""

    def __init__(self, clear_key: bytes | None = None):
        self._handle = _ffi.container.dll.mediaway_demuxer_create()
        if not self._handle:
            raise MediawayError(_ffi.MEDIAWAY_STATUS_INTERNAL_PANIC, "demuxer creation panicked")
        self._time_bases: dict[int, Rational] = {}
        if clear_key is not None:
            self.set_decryption_key(clear_key)

    def push_bytes(self, data: bytes) -> None:
        buf = create_string_buffer(data, len(data))
        _check_container(
            _ffi.container.dll.mediaway_demuxer_push_bytes(self._handle, cast(buf, _ffi.U8P), len(data))
        )

    def stream_count(self) -> int:
        return _ffi.container.dll.mediaway_demuxer_stream_count(self._handle)

    def streams(self) -> list[VideoStreamInfo | AudioStreamInfo]:
        out: list[VideoStreamInfo | AudioStreamInfo] = []
        for index in range(self.stream_count()):
            raw = _ffi.StreamInfo()
            _check_container(_ffi.container.dll.mediaway_demuxer_stream_at(self._handle, index, byref(raw)))
            extra = _copy_bytes(raw.extra_data, raw.extra_data_len)
            _ffi.container.dll.mediaway_stream_info_free(byref(raw))
            time_base = Rational(raw.time_base.num, raw.time_base.den)
            self._time_bases[raw.id] = time_base
            codec = Codec(raw.codec)
            if raw.has_geometry:
                out.append(
                    VideoStreamInfo(
                        codec=codec,
                        width=raw.width,
                        height=raw.height,
                        frame_rate=time_base,
                        extra_data=extra,
                    )
                )
            else:
                out.append(
                    AudioStreamInfo(
                        codec=codec,
                        sample_rate=raw.sample_rate,
                        channels=raw.channels,
                        extra_data=extra,
                    )
                )
        return out

    def poll_packet(self) -> Packet | None:
        raw = _ffi.Packet()
        has = c_bool(False)
        _check_container(_ffi.container.dll.mediaway_demuxer_poll_packet(self._handle, byref(raw), byref(has)))
        if not has.value:
            return None
        payload = _copy_bytes(raw.payload, raw.payload_len)
        _ffi.container.dll.mediaway_packet_free(byref(raw))
        tb = self._time_bases.get(raw.stream_id, Rational(1, 30))
        return Packet(
            stream_index=raw.stream_id,
            pts=_from_units(raw.pts, tb),
            dts=_from_units(raw.dts, tb),
            payload=payload,
            key=raw.is_keyframe,
            duration=_from_units(raw.duration, tb) if raw.duration else None,
        )

    def set_decryption_key(self, key: bytes) -> None:
        if len(key) != 16:
            raise ValueError("decryption key must be exactly 16 bytes")
        buf = create_string_buffer(key, 16)
        _check_container(
            _ffi.container.dll.mediaway_demuxer_set_decryption_key(self._handle, cast(buf, _ffi.U8P), 16)
        )

    def clear_decryption_key(self) -> None:
        _check_container(_ffi.container.dll.mediaway_demuxer_clear_decryption_key(self._handle))

    def __enter__(self) -> "Demuxer":
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.close()

    def close(self) -> None:
        if self._handle:
            _ffi.container.dll.mediaway_demuxer_close(self._handle)
            self._handle = None
