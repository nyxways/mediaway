"""Container capability: Ogg mux + demux (adr/container/0004-ogg-adts-c-abi.md).

Dedicated handles, not `Muxer`/`Demuxer`: Ogg has no track-registration step
and no Open/Live typestate — `OggMuxer` is immediately ready for
`push_packet`. Reuses the shared `PacketView`/`Packet`/`StreamInfo` ABI
structs and frees.
"""

from __future__ import annotations

from ctypes import byref, c_bool, c_size_t, cast, create_string_buffer

from . import _ffi
from ._container import _check_container, _copy_bytes
from ._errors import MediawayError
from ._types import AudioStreamInfo, Codec, Rational, RawPacket, VideoStreamInfo

__all__ = ["OggMuxer", "OggDemuxer"]


class OggMuxer:
    """Muxes packets into Ogg pages for one logical bitstream."""

    def __init__(self, serial: int):
        self._handle = _ffi.container.dll.mediaway_ogg_muxer_create(serial)
        if not self._handle:
            raise MediawayError(_ffi.MEDIAWAY_STATUS_INTERNAL_PANIC, "Ogg muxer creation panicked")

    def push_packet(self, packet: RawPacket) -> None:
        """Write one Ogg page. `packet.pts` becomes the page's granule
        position; `packet.discard` becomes the page's EOS flag. Fails with
        INVALID_DATA if the payload exceeds a single Ogg page's capacity —
        this mux always emits one page per packet."""
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
        _check_container(_ffi.container.dll.mediaway_ogg_muxer_push_packet(self._handle, byref(raw)))

    def flush(self) -> None:
        """No-op — every `push_packet` call already wrote a complete,
        independently valid Ogg page. Exposed for shape parity with `LiveMuxer.flush`."""
        _check_container(_ffi.container.dll.mediaway_ogg_muxer_flush(self._handle))

    def poll_bytes(self) -> bytes | None:
        out_data = _ffi.U8P()
        out_len = c_size_t(0)
        _check_container(
            _ffi.container.dll.mediaway_ogg_muxer_poll_bytes(self._handle, byref(out_data), byref(out_len))
        )
        if out_len.value == 0:
            return None
        data = _copy_bytes(out_data, out_len.value)
        _ffi.container.dll.mediaway_buffer_free(out_data, out_len)
        return data

    def __enter__(self) -> "OggMuxer":
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.close()

    def close(self) -> None:
        if self._handle:
            _ffi.container.dll.mediaway_ogg_muxer_close(self._handle)
            self._handle = None


class OggDemuxer:
    """Feeds Ogg-container bytes in and pulls demuxed packets/stream info back out."""

    def __init__(self):
        self._handle = _ffi.container.dll.mediaway_ogg_demuxer_create()
        if not self._handle:
            raise MediawayError(_ffi.MEDIAWAY_STATUS_INTERNAL_PANIC, "Ogg demuxer creation panicked")

    def push_bytes(self, data: bytes) -> None:
        buf = create_string_buffer(data, len(data))
        _check_container(
            _ffi.container.dll.mediaway_ogg_demuxer_push_bytes(self._handle, cast(buf, _ffi.U8P), len(data))
        )

    def streams(self) -> list[VideoStreamInfo | AudioStreamInfo]:
        """Streams discovered so far — 0 or 1 (Ogg carries a single logical bitstream)."""
        count = _ffi.container.dll.mediaway_ogg_demuxer_stream_count(self._handle)
        out: list[VideoStreamInfo | AudioStreamInfo] = []
        for index in range(count):
            raw = _ffi.StreamInfo()
            _check_container(_ffi.container.dll.mediaway_ogg_demuxer_stream_at(self._handle, index, byref(raw)))
            extra = _copy_bytes(raw.extra_data, raw.extra_data_len)
            _ffi.container.dll.mediaway_stream_info_free(byref(raw))
            codec = Codec(raw.codec)
            if raw.has_geometry:
                out.append(
                    VideoStreamInfo(
                        codec=codec,
                        width=raw.width,
                        height=raw.height,
                        frame_rate=Rational(raw.time_base.num, raw.time_base.den),
                        extra_data=extra,
                    )
                )
            else:
                out.append(
                    AudioStreamInfo(
                        codec=codec, sample_rate=raw.sample_rate, channels=raw.channels, extra_data=extra
                    )
                )
        return out

    def poll_packet(self) -> RawPacket | None:
        raw = _ffi.Packet()
        has = c_bool(False)
        _check_container(_ffi.container.dll.mediaway_ogg_demuxer_poll_packet(self._handle, byref(raw), byref(has)))
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

    def __enter__(self) -> "OggDemuxer":
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.close()

    def close(self) -> None:
        if self._handle:
            _ffi.container.dll.mediaway_ogg_demuxer_close(self._handle)
            self._handle = None
