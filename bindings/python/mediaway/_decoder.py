"""Pipeline capability: auto video decode + Opus audio decode.

Wraps the `mediaway-ffi` C ABI's decode sessions (adr/0004-auto-decode-c-abi.md,
adr/pipeline/0006-audio-decode-c-abi.md) — the exact same "C ABI real, no
language binding wired" gap the container format series closed for mux/demux,
closed here for decode. Both sessions mirror `AutoVideoEncoder`/`AudioEncoder`'s
single-step shape (the handle IS the decoder, no consumption trap); `NO_BACKEND`
raises `DecoderUnavailableError`, an expected/graceful outcome.
"""

from __future__ import annotations

from ctypes import byref, c_bool, c_void_p, cast, create_string_buffer

from . import _ffi
from ._container import _from_units, _to_units
from ._encoder import _check_pipeline, _copy
from ._errors import DecoderUnavailableError
from ._types import Codec, DecodedAudioFrame, DecodedVideoFrame, DecodePacket, PixelFormat, Rational

__all__ = ["DecodeSession", "AudioDecodeSession"]


class DecodeSession:
    """The best available video decoder for a config — the handle IS the
    decoder (single-step open, no consumption trap, mirrors
    `AutoVideoEncoder`'s `NO_BACKEND` handling). CPU output only (GPU decode
    output is deferred, adr/0004 §1/§5).
    """

    def __init__(self, handle: int, time_base: Rational):
        self._handle = handle
        self._time_base = time_base

    @classmethod
    def open(
        cls,
        *,
        codec: Codec = Codec.H264,
        width: int,
        height: int,
        time_base: Rational,
        pixel_format: PixelFormat = PixelFormat.NV12,
        extra_data: bytes = b"",
    ) -> "DecodeSession":
        """Open the best available video decoder for `codec`/`width`/`height`.

        `extra_data` (AVCC / SPS-PPS codec config) is required at open time
        (not supplied via the first pushed packet — see adr/0004 §1 for why
        the muxer-track analogy does not hold for the wrapped decoder).
        Raises `DecoderUnavailableError` when no decode backend exists.
        """
        buf = create_string_buffer(extra_data, len(extra_data)) if extra_data else None
        raw = _ffi.pipeline.dll.mediaway_auto_video_decode_config_new(
            int(codec),
            width,
            height,
            _ffi.Rational(time_base.num, time_base.den),
            cast(buf, _ffi.U8P) if buf else None,
            len(extra_data),
        )
        raw.pixel_format = int(pixel_format)
        out = c_void_p()
        _check_pipeline(
            _ffi.pipeline.dll.mediaway_decode_session_open(byref(raw), byref(out)),
            no_backend_error=DecoderUnavailableError,
        )
        if not out.value:
            raise DecoderUnavailableError(_ffi.PIPELINE_UNKNOWN_ERROR, "decode session open returned no handle")
        return cls(out.value, time_base)

    def push_packet(self, packet: DecodePacket) -> None:
        """Push one compressed packet. May produce zero or more frames
        (drain via `poll_frame()`)."""
        raw = _ffi.DecodePacketView(
            stream_id=0,
            pts=_to_units(packet.pts, self._time_base),
            dts=_to_units(packet.dts if packet.dts is not None else packet.pts, self._time_base),
            duration=_to_units(packet.duration, self._time_base) if packet.duration else 0,
            is_keyframe=packet.key,
            is_discard=False,
            payload=None,
            payload_len=0,
        )
        if packet.payload:
            buf = create_string_buffer(packet.payload, len(packet.payload))
            raw.payload = cast(buf, _ffi.U8P)
            raw.payload_len = len(packet.payload)
        _check_pipeline(_ffi.pipeline.dll.mediaway_decode_session_push_packet(self._handle, byref(raw)))

    def poll_frame(self) -> DecodedVideoFrame | None:
        """Next decoded frame, if ready. `None` is a valid "nothing ready
        yet" result, not an error."""
        raw = _ffi.DecodedVideoFrame()
        has = c_bool(False)
        _check_pipeline(_ffi.pipeline.dll.mediaway_decode_session_poll_frame(self._handle, byref(raw), byref(has)))
        if not has.value:
            return None
        data = _copy(raw.data, raw.data_len)
        frame = DecodedVideoFrame(
            width=raw.width,
            height=raw.height,
            format=PixelFormat(raw.pixel_format),
            data=data,
            pts=_from_units(raw.pts, self._time_base),
            duration=_from_units(raw.duration, self._time_base) if raw.duration else None,
        )
        _ffi.pipeline.dll.mediaway_decoded_video_frame_free(byref(raw))
        return frame

    def flush(self) -> None:
        """Signal end of input; drain the remaining frames with `poll_frame()`."""
        _check_pipeline(_ffi.pipeline.dll.mediaway_decode_session_flush(self._handle))

    def close(self) -> None:
        """Always safe — this surface has no handle-consumption trap."""
        if self._handle:
            _ffi.pipeline.dll.mediaway_decode_session_close(self._handle)
            self._handle = None

    def __enter__(self) -> "DecodeSession":
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.close()


class AudioDecodeSession:
    """An Opus audio decode session — the handle IS the decoder
    (adr/pipeline/0006, mirrors `DecodeSession`'s video shape; no muxer to
    wire, no consumption trap). Cross-platform (`mediaway-sw`, no OS
    dependency), unlike `DecodeSession`'s Windows-only WMF backend.
    """

    def __init__(self, handle: int, sample_rate: int, channels: int, time_base: Rational):
        self._handle = handle
        self.sample_rate = sample_rate
        self.channels = channels
        self._time_base = time_base

    @classmethod
    def open(cls, *, sample_rate: int, channels: int, time_base: Rational) -> "AudioDecodeSession":
        """Open an Opus decode session. Raises `DecoderUnavailableError`
        when no decode backend exists."""
        raw = _ffi.pipeline.dll.mediaway_audio_decode_config_opus(
            sample_rate, channels, _ffi.Rational(time_base.num, time_base.den)
        )
        out = c_void_p()
        _check_pipeline(
            _ffi.pipeline.dll.mediaway_audio_decode_session_open(byref(raw), byref(out)),
            no_backend_error=DecoderUnavailableError,
        )
        if not out.value:
            raise DecoderUnavailableError(
                _ffi.PIPELINE_UNKNOWN_ERROR, "audio decode session open returned no handle"
            )
        return cls(out.value, sample_rate, channels, time_base)

    def push_packet(self, packet: DecodePacket) -> None:
        """Push one compressed Opus packet. An empty `payload` is Opus's
        packet-loss-concealment hint for a lost frame, not an error. May
        produce zero or more frames (drain via `poll_frame()`)."""
        raw = _ffi.DecodePacketView(
            stream_id=0,
            pts=_to_units(packet.pts, self._time_base),
            dts=_to_units(packet.dts if packet.dts is not None else packet.pts, self._time_base),
            duration=_to_units(packet.duration, self._time_base) if packet.duration else 0,
            is_keyframe=packet.key,
            is_discard=False,
            payload=None,
            payload_len=0,
        )
        if packet.payload:
            buf = create_string_buffer(packet.payload, len(packet.payload))
            raw.payload = cast(buf, _ffi.U8P)
            raw.payload_len = len(packet.payload)
        _check_pipeline(_ffi.pipeline.dll.mediaway_audio_decode_session_push_packet(self._handle, byref(raw)))

    def poll_frame(self) -> DecodedAudioFrame | None:
        """Next decoded PCM frame, if ready. `None` is a valid "nothing
        ready yet" result, not an error."""
        raw = _ffi.DecodedAudioFrame()
        has = c_bool(False)
        _check_pipeline(
            _ffi.pipeline.dll.mediaway_audio_decode_session_poll_frame(self._handle, byref(raw), byref(has))
        )
        if not has.value:
            return None
        data = _copy(raw.data, raw.data_len)
        frame = DecodedAudioFrame(
            sample_rate=raw.sample_rate,
            channels=raw.channels,
            data=data,
            pts=_from_units(raw.pts, self._time_base),
            duration=_from_units(raw.duration, self._time_base) if raw.duration else None,
        )
        _ffi.pipeline.dll.mediaway_decoded_audio_frame_free(byref(raw))
        return frame

    def flush(self) -> None:
        """Signal end of input; drain the remaining frames with `poll_frame()`."""
        _check_pipeline(_ffi.pipeline.dll.mediaway_audio_decode_session_flush(self._handle))

    def close(self) -> None:
        """Always safe — this surface has no handle-consumption trap."""
        if self._handle:
            _ffi.pipeline.dll.mediaway_audio_decode_session_close(self._handle)
            self._handle = None

    def __enter__(self) -> "AudioDecodeSession":
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.close()
