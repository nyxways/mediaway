"""Pipeline capability: auto video/audio encode -> fragmented MP4.

Wraps the `mediaway-ffi` C ABI (see `_ffi.py` for the raw layer and
`../README.md` for the DX contract). `EncodeSession(encoder)` takes ownership
of the encoder object; `finish()` is terminal (it consumes the session — no
`close()` after it). The audio encoder (ABI v2, adr/0003) is single-step:
`AudioEncoder.open(...)` returns the encode session directly — no intermediate
handle, no consumption trap, `close()` is always safe.
"""

from __future__ import annotations

from ctypes import byref, c_bool, c_size_t, c_uint16, c_uint32, c_void_p, cast, create_string_buffer
from typing import TYPE_CHECKING

import ctypes

from . import _ffi
from ._container import _from_units, _to_units
from ._errors import EncoderUnavailableError, MediawayError
from ._types import AudioStreamInfo, Codec, Packet, PixelFormat, Rational, SampleFormat, VideoFrame

if TYPE_CHECKING:
    from ._device import GpuDevice, VideoCapture

__all__ = ["AutoVideoEncoder", "EncodeSession", "AudioEncoder"]


def _check_pipeline(status: int, *, no_backend_error: type[MediawayError] = EncoderUnavailableError) -> None:
    if status == _ffi.PIPELINE_OK:
        return
    if status == _ffi.PIPELINE_NO_BACKEND:
        raise no_backend_error(status, "no backend compiled in or openable on this platform")
    names = {
        _ffi.PIPELINE_INVALID_ARGUMENT: "invalid argument",
        _ffi.PIPELINE_HANDLE_POISONED: "handle poisoned by an earlier panic",
        _ffi.PIPELINE_UNSUPPORTED: "codec/pixel-format/geometry not supported",
        _ffi.PIPELINE_INVALID_INPUT: "bad dimensions, rates, or frame metadata",
        _ffi.PIPELINE_ENCODER_BACKEND_FAILURE: "encoder backend OS/API failure",
        _ffi.PIPELINE_ENCODER_CLOSED: "session already finished or not open",
        _ffi.PIPELINE_MUX_INVALID_TRACK: "muxer rejected the encoder's stream info",
        _ffi.PIPELINE_MUX_INVALID_PACKET: "packet does not match the registered track",
        _ffi.PIPELINE_MUX_INVALID_DATA: "malformed container data",
        _ffi.PIPELINE_UNKNOWN_ERROR: "unknown error",
        _ffi.PIPELINE_INTERNAL_PANIC: "internal panic (handle poisoned)",
        _ffi.PIPELINE_DECODER_BACKEND_FAILURE: "decoder backend OS/API failure",
        _ffi.PIPELINE_DECODER_CLOSED: "decode session already finished or not open",
    }
    raise MediawayError(status, names.get(status, "unknown status"))


class AutoVideoEncoder:
    """An opened auto encoder: the best available backend for the config.

    Handed to `EncodeSession(encoder)` once — ownership transfers there.
    The ABI does not expose which concrete backend was chosen, so `.name`
    is "auto"; `.codec` reports the requested codec.
    """

    def __init__(self, handle: int, codec: Codec, time_base: Rational):
        self._handle = handle
        self.codec = codec
        self._time_base = time_base
        self.name = "auto"  # the concrete OS/GPU backend is not exposed by the ABI

    @classmethod
    def pick(
        cls,
        *,
        codec: Codec = Codec.H264,
        width: int,
        height: int,
        frame_rate: Rational,
        pixel_format: PixelFormat = PixelFormat.NV12,
        bitrate_bps: int = 0,
        gpu_device: "GpuDevice | None" = None,
    ) -> "AutoVideoEncoder":
        """Open the best available encoder for the config; raises
        EncoderUnavailableError when no backend exists on this machine.

        `gpu_device` opts into the Zero-Copy/GPU-copy input path used by the
        capture-to-encode bridge (`write_frame_from_desktop_capture`) — pass
        the same `GpuDevice` the capture was opened with, and set
        `pixel_format=PixelFormat.BGRA8` for Screen (DXGI Desktop Duplication
        delivers BGRA8, not this method's NV12 default)."""
        raw = _ffi.pipeline.dll.mediaway_auto_video_encode_config_new(
            int(codec), width, height, _ffi.Rational(frame_rate.num, frame_rate.den)
        )
        raw.pixel_format = int(pixel_format)
        raw.bitrate_bps = bitrate_bps
        if gpu_device is not None:
            raw.gpu_device = gpu_device.handle
        out = c_void_p()
        _check_pipeline(_ffi.pipeline.dll.mediaway_auto_encoder_open(byref(raw), byref(out)))
        if not out.value:
            raise MediawayError(_ffi.PIPELINE_UNKNOWN_ERROR, "encoder open returned no handle")
        return cls(out.value, codec, frame_rate)

    def close(self) -> None:
        """Abandon this encoder without opening a session (early-abort path)."""
        if self._handle:
            _ffi.pipeline.dll.mediaway_auto_encoder_close(self._handle)
            self._handle = None


class EncodeSession:
    """Registers the encoder's stream as an MP4 track and begins streaming.

    Takes ownership of `encoder` unconditionally (success or failure). Use
    `finish()` to flush and collect the complete fMP4 bytes — it consumes the
    session; `close()` after it is a no-op.
    """

    def __init__(self, encoder: AutoVideoEncoder):
        out = c_void_p()
        _check_pipeline(_ffi.pipeline.dll.mediaway_encode_session_open(encoder._handle, byref(out)))
        encoder._handle = None  # consumed by session_open unconditionally
        if not out.value:
            raise MediawayError(_ffi.PIPELINE_UNKNOWN_ERROR, "session open returned no handle")
        self._handle = out.value
        self._time_base = encoder._time_base

    def push_frame(self, frame: VideoFrame) -> None:
        raw = _ffi.VideoFrame(
            pts=_to_units(frame.pts, self._time_base),
            duration=_to_units(frame.duration, self._time_base) if frame.duration else 0,
            width=frame.width,
            height=frame.height,
            pixel_format=int(frame.format),
            storage_kind=_ffi.STORAGE_CPU,
            raw_bytes=None,
            raw_bytes_len=0,
            gpu_buffer=_ffi.GpuBufferHandle(
                kind=_ffi.GPU_BUFFER_UNKNOWN, native_a=0, native_b=0, subresource=0, webgpu_texture_id=0
            ),
        )
        if frame.data:
            buf = create_string_buffer(frame.data, len(frame.data))
            raw.raw_bytes = cast(buf, _ffi.U8P)
            raw.raw_bytes_len = len(frame.data)
        _check_pipeline(_ffi.pipeline.dll.mediaway_encode_session_write_frame(self._handle, byref(raw)))

    def write_frame_from_camera_capture(self, capture: "VideoCapture") -> bool:
        """Poll one frame from `capture` and, if one was ready, push it
        straight into the encoder in a single native call — no intermediate
        frame struct crosses the FFI boundary
        (adr/pipeline/0005-capture-encode-bridge-c-abi.md). Returns False (a
        no-op) when no frame was ready yet, mirroring `VideoCapture.poll_frame`'s
        own contract. `capture` must be a session opened via
        `VideoCapture.open(source="camera")`."""
        wrote = c_bool(False)
        _check_pipeline(
            _ffi.pipeline.dll.mediaway_encode_session_write_frame_from_camera_capture(
                self._handle, capture._handle, byref(wrote)
            )
        )
        return bool(wrote.value)

    def write_frame_from_desktop_capture(self, capture: "VideoCapture") -> bool:
        """Same bridge as `write_frame_from_camera_capture`, but for Screen's
        GPU-only frames — Zero-Copy, no CPU copy. `capture` must be a session
        opened via `VideoCapture.open(source="screen")`."""
        wrote = c_bool(False)
        _check_pipeline(
            _ffi.pipeline.dll.mediaway_encode_session_write_frame_from_desktop_capture(
                self._handle, capture._handle, byref(wrote)
            )
        )
        return bool(wrote.value)

    def finish(self) -> bytes:
        """Flush the encoder + muxer and return the complete fMP4 bytes. Terminal."""
        out_data = _ffi.U8P()
        out_len = c_size_t(0)
        _check_pipeline(_ffi.pipeline.dll.mediaway_encode_session_finish(self._handle, byref(out_data), byref(out_len)))
        self._handle = None  # consumed by finish unconditionally
        length = out_len.value
        if length == 0:
            return b""
        data = _copy(out_data, length)
        _ffi.pipeline.dll.mediaway_pipeline_ffi_buffer_free(out_data, out_len)
        return data

    def close(self) -> None:
        """Abandon the session without finishing (no valid MP4 output)."""
        if self._handle:
            _ffi.pipeline.dll.mediaway_encode_session_close(self._handle)
            self._handle = None

    def __enter__(self) -> "EncodeSession":
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        if exc_type is None:
            # Normal exit: finish() should have consumed the session already;
            # if the user forgot, close to avoid leaking the handle.
            self.close()
        else:
            self.close()


def _copy(ptr, length: int) -> bytes:
    if not ptr or length == 0:
        return b""
    return ctypes.string_at(ptr, length)


class AudioEncoder:
    """An opened auto audio encoder — the session IS the encoder (ABI v2,
    adr/0003): single-step open, no intermediate handle, no consumption trap.

    `open()` raises `EncoderUnavailableError` when no audio backend exists.
    Push interleaved F32 PCM, poll AAC packets back, and register the muxer
    audio track with `stream_info()` (the AudioSpecificConfig materializes
    after the first pushed frame — adr/0003 call order: push, then stream_info,
    then mux).
    """

    def __init__(self, handle: int, sample_rate: int, channels: int, time_base: Rational):
        self._handle = handle
        self.sample_rate = sample_rate
        self.channels = channels
        self._time_base = time_base
        self._next_pts = 0  # running sample index in the stream time base
        self.name = "auto"  # the concrete backend is not exposed by the ABI

    @classmethod
    def open(
        cls,
        *,
        codec: Codec = Codec.AAC,
        sample_rate: int,
        channels: int,
        time_base: Rational | None = None,
        bitrate_bps: int = 0,
    ) -> "AudioEncoder":
        """Open the best available audio encoder.

        `sample_rate`/`channels` must match the PCM frames pushed afterward
        (e.g. the mic's negotiated values — the AAC sugar defaults to stereo,
        which a mono mic is not).
        """
        tb = time_base or Rational(1, sample_rate)
        raw = _ffi.AudioEncodeConfig(
            codec=int(codec),
            sample_rate=sample_rate,
            channels=channels,
            sample_format=int(SampleFormat.F32),
            time_base=_ffi.Rational(tb.num, tb.den),
            bitrate_bps=bitrate_bps,
        )
        out = c_void_p()
        _check_pipeline(_ffi.pipeline.dll.mediaway_audio_encoder_open(byref(raw), byref(out)))
        if not out.value:
            raise MediawayError(_ffi.PIPELINE_UNKNOWN_ERROR, "audio encoder open returned no handle")
        return cls(out.value, sample_rate, channels, tb)

    def push_pcm(self, data: bytes, pts: Rational | None = None) -> None:
        """Push one interleaved F32 PCM chunk (e.g. bytes from
        `AudioCapture.poll_pcm()`). `pts` defaults to the running sample
        index (consecutive frames are contiguous in the stream time base);
        the duration is derived from the chunk length and negotiated channels.
        """
        if pts is not None:
            self._next_pts = _to_units(pts, self._time_base)
        samples = len(data) // (4 * self.channels)
        buf = create_string_buffer(data, len(data)) if data else None
        raw = _ffi.AudioFrameView(
            pts=self._next_pts,
            duration=samples,
            sample_rate=self.sample_rate,
            channels=self.channels,
            sample_format=int(SampleFormat.F32),
            data=cast(buf, _ffi.U8P) if buf else None,
            data_len=len(data),
        )
        _check_pipeline(_ffi.pipeline.dll.mediaway_audio_encode_session_push_pcm(self._handle, byref(raw)))
        self._next_pts += samples

    def poll_packet(self) -> Packet | None:
        """Next encoded packet, if ready. `stream_index` is 0 — set it to the
        muxer-assigned audio track id before pushing."""
        raw = _ffi.AudioPacket()
        has = c_bool(False)
        _check_pipeline(
            _ffi.pipeline.dll.mediaway_audio_encode_session_poll_packet(self._handle, byref(raw), byref(has))
        )
        if not has.value:
            return None
        payload = _copy(raw.payload, raw.payload_len)
        _ffi.pipeline.dll.mediaway_pipeline_ffi_packet_free(byref(raw))
        return Packet(
            stream_index=0,
            pts=_from_units(raw.pts, self._time_base),
            payload=payload,
            key=bool(raw.is_keyframe),
            duration=_from_units(raw.duration, self._time_base) if raw.duration else None,
        )

    def flush(self) -> None:
        """Signal end of input; drain the remaining packets with `poll_packet()`."""
        _check_pipeline(_ffi.pipeline.dll.mediaway_audio_encode_session_flush(self._handle))

    def stream_info(self) -> AudioStreamInfo:
        """Codec config (`AudioSpecificConfig`) + negotiated rates — available
        after the first pushed frame."""
        raw = _ffi.AudioStreamInfo()
        _check_pipeline(
            _ffi.pipeline.dll.mediaway_audio_encode_session_stream_info(self._handle, byref(raw))
        )
        extra = _copy(raw.extra_data, raw.extra_data_len)
        _ffi.pipeline.dll.mediaway_pipeline_ffi_stream_info_free(byref(raw))
        return AudioStreamInfo(
            codec=Codec(raw.codec),
            sample_rate=raw.sample_rate,
            channels=raw.channels,
            extra_data=extra,
        )

    def close(self) -> None:
        """Always safe — this surface has no handle-consumption trap."""
        if self._handle:
            _ffi.pipeline.dll.mediaway_audio_encode_session_close(self._handle)
            self._handle = None

    def __enter__(self) -> "AudioEncoder":
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.close()
