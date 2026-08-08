"""Container capability: MP3 (MPEG Layer III) mux + demux (adr/container/0007-mp3-c-abi.md).

A fixed header for the mux session's lifetime (no track registration at
all); `write_frame` takes an explicit `padding` bit no `RawPacket` has a slot for.
"""

from __future__ import annotations

from ctypes import byref, c_bool, c_size_t, cast, create_string_buffer

from . import _ffi
from ._container import _check_container, _copy_bytes
from ._errors import MediawayError
from ._types import AudioStreamInfo, Codec, Mp3FrameHeader, RawPacket

__all__ = ["Mp3Muxer", "Mp3Demuxer"]


class Mp3Muxer:
    """Appends encoded MPEG Layer III frames."""

    def __init__(self, header: Mp3FrameHeader):
        """`header` must be a standard Layer III bitrate/sample-rate combination for its version."""
        raw = _ffi.Mp3FrameHeader(
            version=int(header.version),
            bitrate_kbps=header.bitrate_kbps,
            sample_rate=header.sample_rate,
            channel_mode=int(header.channel_mode),
        )
        self._handle = _ffi.container.dll.mediaway_mp3_muxer_create(byref(raw))
        if not self._handle:
            raise MediawayError(
                _ffi.MEDIAWAY_STATUS_INVALID_ARGUMENT,
                "non-standard MP3 bitrate/sample-rate combination, or the native call panicked",
            )

    def write_frame(self, frame_body: bytes, padding: bool) -> bytes:
        """Append one already-encoded Layer III frame body. Fails with
        INVALID_PACKET when `frame_body`'s length doesn't match what the
        header's bitrate/sample-rate/padding combination requires."""
        buf = create_string_buffer(frame_body, len(frame_body)) if frame_body else None
        payload = cast(buf, _ffi.U8P) if buf is not None else _ffi.U8P()
        out_data = _ffi.U8P()
        out_len = c_size_t(0)
        _check_container(
            _ffi.container.dll.mediaway_mp3_muxer_write_frame(
                self._handle, payload, len(frame_body), padding, byref(out_data), byref(out_len)
            )
        )
        data = _copy_bytes(out_data, out_len.value)
        _ffi.container.dll.mediaway_buffer_free(out_data, out_len)
        return data

    def __enter__(self) -> "Mp3Muxer":
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.close()

    def close(self) -> None:
        if self._handle:
            _ffi.container.dll.mediaway_mp3_muxer_close(self._handle)
            self._handle = None


class Mp3Demuxer:
    """Feeds MPEG audio elementary-stream bytes in and pulls demuxed Layer III frames back out."""

    def __init__(self):
        self._handle = _ffi.container.dll.mediaway_mp3_demuxer_create()
        if not self._handle:
            raise MediawayError(_ffi.MEDIAWAY_STATUS_INTERNAL_PANIC, "MP3 demuxer creation panicked")

    def push_bytes(self, data: bytes) -> None:
        buf = create_string_buffer(data, len(data))
        _check_container(
            _ffi.container.dll.mediaway_mp3_demuxer_push_bytes(self._handle, cast(buf, _ffi.U8P), len(data))
        )

    def streams(self) -> list[AudioStreamInfo]:
        """Streams discovered so far — 0 or 1 (MP3 carries a single implicit stream)."""
        count = _ffi.container.dll.mediaway_mp3_demuxer_stream_count(self._handle)
        out: list[AudioStreamInfo] = []
        for index in range(count):
            raw = _ffi.StreamInfo()
            _check_container(_ffi.container.dll.mediaway_mp3_demuxer_stream_at(self._handle, index, byref(raw)))
            extra = _copy_bytes(raw.extra_data, raw.extra_data_len)
            _ffi.container.dll.mediaway_stream_info_free(byref(raw))
            out.append(
                AudioStreamInfo(
                    codec=Codec(raw.codec), sample_rate=raw.sample_rate, channels=raw.channels, extra_data=extra
                )
            )
        return out

    def poll_packet(self) -> RawPacket | None:
        """Pop the next demuxed Layer III frame. pts/duration are synthesized
        from a running samples-per-frame count — MPEG audio carries no
        per-frame timing of its own."""
        raw = _ffi.Packet()
        has = c_bool(False)
        _check_container(_ffi.container.dll.mediaway_mp3_demuxer_poll_packet(self._handle, byref(raw), byref(has)))
        if not has.value:
            return None
        payload = _copy_bytes(raw.payload, raw.payload_len)
        _ffi.container.dll.mediaway_packet_free(byref(raw))
        return RawPacket(
            stream_id=raw.stream_id,
            pts=raw.pts,
            dts=raw.dts,
            duration=raw.duration,
            key=raw.is_keyframe,
            discard=raw.is_discard,
            payload=payload,
        )

    def __enter__(self) -> "Mp3Demuxer":
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.close()

    def close(self) -> None:
        if self._handle:
            _ffi.container.dll.mediaway_mp3_demuxer_close(self._handle)
            self._handle = None
