"""Container capability: WAV mux + one-shot parse (adr/container/0008-wav-c-abi.md).

`wav::Muxer::finish` consumes `self` by value on the Rust side (RIFF chunk
sizes must be known up front), so there is no `poll_bytes` step — `finish()`
returns the complete byte stream directly. Demux has NO handle at all:
`parse()` is a one-shot whole-buffer function, unlike every other format in
this package.
"""

from __future__ import annotations

from ctypes import byref, c_size_t, cast, create_string_buffer

from . import _ffi
from ._container import _check_container, _copy_bytes
from ._errors import MediawayError
from ._types import AudioStreamInfo, Codec, RawPacket, VideoStreamInfo, WaveFormat

__all__ = ["WavMuxer", "parse"]


class WavMuxer:
    """Appends raw PCM and finalizes a complete RIFF/WAVE byte stream."""

    def __init__(self, sample_rate: int, channels: int, bits_per_sample: int):
        """Start an integer-PCM mux session. Use `from_format` for an explicit format (e.g. IEEE float)."""
        self._handle = _ffi.container.dll.mediaway_wav_muxer_create(sample_rate, channels, bits_per_sample)
        if not self._handle:
            raise MediawayError(_ffi.MEDIAWAY_STATUS_INTERNAL_PANIC, "WAV muxer creation panicked")
        self._finished = False

    @classmethod
    def from_format(cls, format: WaveFormat) -> "WavMuxer":
        """Start a mux session for an explicit format (e.g. IEEE float PCM)."""
        instance = cls.__new__(cls)
        raw = _ffi.WaveFormat(
            sample_format=int(format.sample_format),
            channels=format.channels,
            sample_rate=format.sample_rate,
            bits_per_sample=format.bits_per_sample,
        )
        instance._handle = _ffi.container.dll.mediaway_wav_muxer_create_with_format(byref(raw))
        if not instance._handle:
            raise MediawayError(_ffi.MEDIAWAY_STATUS_INTERNAL_PANIC, "WAV muxer creation panicked")
        instance._finished = False
        return instance

    def push_packet(self, packet: RawPacket) -> None:
        """Append raw interleaved PCM bytes, already encoded per the session's format."""
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
        _check_container(_ffi.container.dll.mediaway_wav_muxer_push_packet(self._handle, byref(raw)))

    def finish(self) -> bytes:
        """Finalize the mux session and return the complete RIFF/WAVE byte
        stream. Only the native-side internal state is consumed — this
        `WavMuxer` stays usable for `close()` afterward. A second call fails
        with INVALID_STATE rather than re-finalizing."""
        out_data = _ffi.U8P()
        out_len = c_size_t(0)
        _check_container(
            _ffi.container.dll.mediaway_wav_muxer_finish(self._handle, byref(out_data), byref(out_len))
        )
        self._finished = True
        data = _copy_bytes(out_data, out_len.value)
        _ffi.container.dll.mediaway_buffer_free(out_data, out_len)
        return data

    @property
    def is_finished(self) -> bool:
        return self._finished

    def __enter__(self) -> "WavMuxer":
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.close()

    def close(self) -> None:
        if self._handle:
            _ffi.container.dll.mediaway_wav_muxer_close(self._handle)
            self._handle = None


def parse(data: bytes) -> tuple[VideoStreamInfo | AudioStreamInfo, RawPacket]:
    """Parse a complete RIFF/WAVE buffer into its single track's stream info
    and one packet holding the whole PCM payload."""
    buf = create_string_buffer(data, len(data))
    out_info = _ffi.StreamInfo()
    out_packet = _ffi.Packet()
    _check_container(
        _ffi.container.dll.mediaway_wav_parse(cast(buf, _ffi.U8P), len(data), byref(out_info), byref(out_packet))
    )
    extra = _copy_bytes(out_info.extra_data, out_info.extra_data_len)
    info: VideoStreamInfo | AudioStreamInfo = AudioStreamInfo(
        codec=Codec(out_info.codec), sample_rate=out_info.sample_rate, channels=out_info.channels, extra_data=extra
    )
    _ffi.container.dll.mediaway_stream_info_free(byref(out_info))

    payload = _copy_bytes(out_packet.payload, out_packet.payload_len)
    packet = RawPacket(
        stream_id=out_packet.stream_id,
        pts=out_packet.pts,
        dts=out_packet.dts,
        duration=out_packet.duration,
        key=out_packet.is_keyframe,
        discard=out_packet.is_discard,
        payload=payload,
    )
    _ffi.container.dll.mediaway_packet_free(byref(out_packet))
    return info, packet
