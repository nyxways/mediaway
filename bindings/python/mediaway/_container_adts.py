"""Container capability: ADTS mux + demux (adr/container/0004-ogg-adts-c-abi.md).

Same dedicated-handle reasoning as `_container_ogg.py`: ADTS has no
track-registration step and no Open/Live typestate.
"""

from __future__ import annotations

from ctypes import byref, c_bool, c_size_t, cast, create_string_buffer

from . import _ffi
from ._container import _check_container, _copy_bytes
from ._errors import MediawayError
from ._types import AudioStreamInfo, Codec, RawPacket

__all__ = ["AdtsMuxer", "AdtsDemuxer"]


class AdtsMuxer:
    """Wraps raw AAC frames in ADTS headers."""

    def __init__(self, sample_rate: int, channels: int):
        self._handle = _ffi.container.dll.mediaway_adts_muxer_create(sample_rate, channels)
        if not self._handle:
            raise MediawayError(
                _ffi.MEDIAWAY_STATUS_INVALID_ARGUMENT,
                f"non-standard ADTS sample rate ({sample_rate} Hz), or the native call panicked",
            )

    def push_packet(self, packet: RawPacket) -> None:
        """Append one AAC frame (raw, ADTS header added). Fails with
        INVALID_PACKET if the payload is too large for ADTS's 13-bit
        frame-length field."""
        raw = _ffi.PacketView(
            stream_id=packet.stream_id,
            pts=packet.pts,
            dts=packet.dts,
            duration=packet.duration,
            is_keyframe=packet.key,
            is_discard=packet.discard,
            payload=None,
            payload_len=0,
        )
        if packet.payload:
            buf = create_string_buffer(packet.payload, len(packet.payload))
            raw.payload = cast(buf, _ffi.U8P)
            raw.payload_len = len(packet.payload)
        _check_container(_ffi.container.dll.mediaway_adts_muxer_push_packet(self._handle, byref(raw)))

    def flush(self) -> None:
        """No-op — ADTS frames are independently appendable. Exposed for shape parity."""
        _check_container(_ffi.container.dll.mediaway_adts_muxer_flush(self._handle))

    def poll_bytes(self) -> bytes | None:
        out_data = _ffi.U8P()
        out_len = c_size_t(0)
        _check_container(
            _ffi.container.dll.mediaway_adts_muxer_poll_bytes(self._handle, byref(out_data), byref(out_len))
        )
        if out_len.value == 0:
            return None
        data = _copy_bytes(out_data, out_len.value)
        _ffi.container.dll.mediaway_buffer_free(out_data, out_len)
        return data

    def __enter__(self) -> "AdtsMuxer":
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.close()

    def close(self) -> None:
        if self._handle:
            _ffi.container.dll.mediaway_adts_muxer_close(self._handle)
            self._handle = None


class AdtsDemuxer:
    """Feeds ADTS elementary-stream bytes in and pulls demuxed AAC frames back out."""

    def __init__(self):
        self._handle = _ffi.container.dll.mediaway_adts_demuxer_create()
        if not self._handle:
            raise MediawayError(_ffi.MEDIAWAY_STATUS_INTERNAL_PANIC, "ADTS demuxer creation panicked")

    def push_bytes(self, data: bytes) -> None:
        buf = create_string_buffer(data, len(data))
        _check_container(
            _ffi.container.dll.mediaway_adts_demuxer_push_bytes(self._handle, cast(buf, _ffi.U8P), len(data))
        )

    def streams(self) -> list[AudioStreamInfo]:
        """Streams discovered so far — 0 or 1 (ADTS carries a single implicit stream)."""
        count = _ffi.container.dll.mediaway_adts_demuxer_stream_count(self._handle)
        out: list[AudioStreamInfo] = []
        for index in range(count):
            raw = _ffi.StreamInfo()
            _check_container(_ffi.container.dll.mediaway_adts_demuxer_stream_at(self._handle, index, byref(raw)))
            extra = _copy_bytes(raw.extra_data, raw.extra_data_len)
            _ffi.container.dll.mediaway_stream_info_free(byref(raw))
            out.append(
                AudioStreamInfo(
                    codec=Codec(raw.codec), sample_rate=raw.sample_rate, channels=raw.channels, extra_data=extra
                )
            )
        return out

    def poll_packet(self) -> RawPacket | None:
        """Pop the next demuxed AAC frame. pts/duration are synthesized from a
        running 1024-samples-per-frame count — ADTS carries no per-frame
        timing of its own."""
        raw = _ffi.Packet()
        has = c_bool(False)
        _check_container(_ffi.container.dll.mediaway_adts_demuxer_poll_packet(self._handle, byref(raw), byref(has)))
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

    def __enter__(self) -> "AdtsDemuxer":
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.close()

    def close(self) -> None:
        if self._handle:
            _ffi.container.dll.mediaway_adts_demuxer_close(self._handle)
            self._handle = None
