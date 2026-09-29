"""Mediaway error types.

The C ABI reports failures as per-crate status enums with no exceptions
crossing the boundary. This module translates those statuses into idiomatic
Python exceptions; examples never check raw status codes.

Expected, graceful outcomes get distinct subclasses so example code can
catch-and-continue on missing hardware instead of crashing:
  - EncoderUnavailableError  — no encode backend compiled in / openable
  - DeviceUnavailableError   — no capture backend / device present
  - CaptureUnsupportedError  — the ABI returned UNSUPPORTED for this config
    (today: Window capture — has no C constructor this pass; Screen capture
    is real, see `GpuDevice`/`VideoCapture.open(source="screen")`)
"""

from __future__ import annotations

__all__ = [
    "MediawayError",
    "EncoderUnavailableError",
    "DecoderUnavailableError",
    "DeviceUnavailableError",
    "CaptureUnsupportedError",
    "InvalidStateError",
    "UnknownStreamError",
    "OutOfOrderPacketError",
]


class MediawayError(Exception):
    """A Mediaway C ABI call returned a non-OK status.

    `status` carries the raw per-crate status value; `message` is a
    human-readable English description of that status.
    """

    def __init__(self, status: int, message: str | None = None):
        self.status = status
        super().__init__(message or f"Mediaway error (status {status})")


class EncoderUnavailableError(MediawayError):
    """No video encoder backend could be opened for the requested config.

    Maps the pipeline ABI's NO_BACKEND (and unsupported-codec) outcomes.
    Expected on machines without a usable encoder — catch it and exit
    gracefully rather than crashing.
    """


class DecoderUnavailableError(MediawayError):
    """No decoder backend could be opened for the requested config.

    Maps the pipeline ABI's NO_BACKEND outcome for `DecodeSession`/
    `AudioDecodeSession.open()`. Expected on machines without a usable
    decoder — catch it and exit gracefully rather than crashing.
    """


class DeviceUnavailableError(MediawayError):
    """A capture device or backend could not be opened.

    Maps the device ABI's NO_BACKEND / BACKEND_FAILURE / ACCESS_DENIED
    outcomes for video/audio capture opens.
    """


class CaptureUnsupportedError(MediawayError):
    """The ABI rejected this capture configuration as unsupported.

    Maps the device ABI's UNSUPPORTED outcome — today this is Window capture
    (`VideoCapture.open(source="window")`, no C constructor this pass). Not a
    bug: a documented capability gap. Screen capture is real (see `GpuDevice`)
    and does not raise this.
    """


class InvalidStateError(MediawayError):
    """A call violated a handle's typestate (e.g. add_track after begin()).

    Maps the container ABI's INVALID_STATE outcome. The wrappers make most of
    these unrepresentable, but defensive examples may still hit them.
    """


class UnknownStreamError(MediawayError):
    """A packet's stream id was never added to the `ReplayRing`.

    Maps the container ABI's UNKNOWN_STREAM outcome. Call
    `ReplayRing.add_stream` for the stream first.
    """


class OutOfOrderPacketError(MediawayError):
    """A `ReplayRing` stream's decode timestamp went backwards.

    Maps the container ABI's INVALID_PACKET outcome for the replay ring. The
    ring is decode-ordered per stream, so the packet was NOT added; the caller
    can drop it and carry on. The ring itself is unharmed.
    """
