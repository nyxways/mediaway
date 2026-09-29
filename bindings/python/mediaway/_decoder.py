"""Pipeline capability: auto video decode + Opus/AAC audio decode.

Wraps the `mediaway-ffi` C ABI's decode sessions (adr/0004-auto-decode-c-abi.md,
adr/pipeline/0006-audio-decode-c-abi.md, adr/pipeline/0007 for AAC) — the exact same "C ABI real, no
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
    """An Opus or AAC audio decode session — the handle IS the decoder
    (adr/pipeline/0006, mirrors `DecodeSession`'s video shape; no muxer to
    wire, no consumption trap).

    **Opus** is the software decoder (`mediaway-sw`): identical output on every
    host. **AAC** (adr/pipeline/0007 §2) is the OS's own decoder — Media
    Foundation on Windows, AudioToolbox on macOS/iOS, unavailable elsewhere —
    so, unlike Opus, its samples can differ between hosts and OS versions.
    The Apple arm is compile-checked in this workspace but has not been run.
    Call `decoder_support(Codec.AAC)` to learn whether it exists here.
    """

    def __init__(self, handle: int, sample_rate: int, channels: int, time_base: Rational):
        self._handle = handle
        self.sample_rate = sample_rate
        self.channels = channels
        self._time_base = time_base

    @classmethod
    def open(
        cls,
        *,
        sample_rate: int,
        channels: int,
        time_base: Rational,
        codec: Codec = Codec.OPUS,
        extra_data: bytes = b"",
    ) -> "AudioDecodeSession":
        """Open an Opus (default) or AAC decode session.

        For `Codec.AAC`, `extra_data` is the stream's raw `AudioSpecificConfig`
        (an MP4's `esds` DecoderSpecificInfo, i.e. `AudioStreamInfo.extra_data`)
        and is **required**: an empty one raises `MediawayError` with
        `PIPELINE_INVALID_INPUT`, because a synthesised default would decode
        SBR/PS streams to quietly wrong output. Raw AAC only — de-header ADTS
        first. `time_base` is normally `Rational(1, sample_rate)`, so packet
        and frame timestamps are sample counts. Opus takes no `extra_data`.

        Raises `DecoderUnavailableError` when no decode backend exists, which
        includes AAC on a platform with no OS AAC decoder.
        """
        tb = _ffi.Rational(time_base.num, time_base.den)
        # The borrowed ASC must stay alive until open() returns (`buf` is a local).
        buf = create_string_buffer(extra_data, len(extra_data)) if extra_data else None
        if codec == Codec.OPUS:
            raw = _ffi.pipeline.dll.mediaway_audio_decode_config_opus(sample_rate, channels, tb)
        elif codec == Codec.AAC:
            raw = _ffi.pipeline.dll.mediaway_audio_decode_config_aac(
                sample_rate, channels, tb, cast(buf, _ffi.U8P) if buf else None, len(extra_data)
            )
        else:
            raise DecoderUnavailableError(_ffi.PIPELINE_UNSUPPORTED, f"audio decode supports Opus and AAC, not {codec!r}")
        out = c_void_p()
        status = _ffi.pipeline.dll.mediaway_audio_decode_session_open(byref(raw), byref(out))
        if codec == Codec.AAC and status == _ffi.PIPELINE_UNSUPPORTED:
            # No OS AAC decoder on this platform: an expected, catch-and-continue outcome.
            raise DecoderUnavailableError(status, "no AAC decoder on this platform")
        _check_pipeline(status, no_backend_error=DecoderUnavailableError)
        if not out.value:
            raise DecoderUnavailableError(
                _ffi.PIPELINE_UNKNOWN_ERROR, "audio decode session open returned no handle"
            )
        return cls(out.value, sample_rate, channels, time_base)

    def push_packet(self, packet: DecodePacket) -> None:
        """Push one compressed packet. For Opus an empty `payload` is the
        packet-loss-concealment hint for a lost frame, not an error; for AAC
        it means nothing and raises `MediawayError` (`PIPELINE_INVALID_INPUT`).
        May produce zero or more frames (drain via `poll_frame()`)."""
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
