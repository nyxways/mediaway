"""Pipeline capability probes: which encoders / decoders work on this machine.

Wraps `mediaway_encoder_support_at` / `mediaway_decoder_support`
(adr/pipeline/0007-stream-bytes-aac-decode-support-probe.md §3).

**Both probes are costly.** Each opens throwaway sessions (a real MFT / VA-API /
VideoToolbox session per row), so call them when a settings screen opens —
never per frame or in a loop.
"""

from __future__ import annotations

from ctypes import POINTER, byref, c_int32, c_size_t

from . import _ffi
from ._encoder import _check_pipeline
from ._types import Codec, EncodeBackend, EncodePathClass, EncoderCapability, SupportState

__all__ = ["encoder_support", "decoder_support"]


def _enum(cls, value: int):
    """Map a raw ABI value to `cls`, falling back to `cls.UNKNOWN` for one added later."""
    try:
        return cls(value)
    except ValueError:
        return cls.UNKNOWN


def encoder_support(codec: Codec, width: int, height: int) -> list[EncoderCapability]:
    """Probe every encode backend for `codec` **at `width` x `height`**.

    Encoder support is resolution-dependent — a hardware encoder has minimum
    and maximum dimensions, so a resolution-free answer would be wrong. Pass
    the size you will encode. A platform with no per-backend selection returns
    an empty list. Costly: see the module docs.

    Raises `MediawayError` (`PIPELINE_INVALID_INPUT`) for a zero width/height.
    """
    rows = POINTER(_ffi.EncoderCapability)()
    count = c_size_t(0)
    _check_pipeline(
        _ffi.pipeline.dll.mediaway_encoder_support_at(int(codec), width, height, byref(rows), byref(count))
    )
    try:
        return [
            EncoderCapability(
                backend=_enum(EncodeBackend, rows[i].backend),
                state=_enum(SupportState, rows[i].state),
                path_class=_enum(EncodePathClass, rows[i].path_class),
            )
            for i in range(count.value)
        ]
    finally:
        _ffi.pipeline.dll.mediaway_encoder_support_free(rows, count)


def decoder_support(codec: Codec) -> SupportState:
    """Whether decoding `codec` is usable on this machine right now.

    Decode has one implementation per platform, so this is one state, not a
    list. It is how to learn whether AAC decode exists here before opening a
    session. Costly: see the module docs.
    """
    state = c_int32(0)
    _check_pipeline(_ffi.pipeline.dll.mediaway_decoder_support(int(codec), byref(state)))
    return _enum(SupportState, state.value)
