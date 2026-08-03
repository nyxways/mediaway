"""Mediaway error types.

The C ABI reports failures as per-crate status enums with no exceptions
crossing the boundary. This module translates those statuses into idiomatic
Python exceptions; examples never check raw status codes.

Expected, graceful outcomes get distinct subclasses so example code can
catch-and-continue on missing hardware instead of crashing:
  - EncoderUnavailableError  — no encode backend compiled in / openable
  - DeviceUnavailableError   — no capture backend / device present
  - CaptureUnsupportedError  — the ABI returned UNSUPPORTED for this config
    (today: Screen capture from the C ABI — needs a GPU device handle with no
    C representation yet)
"""

from __future__ import annotations

__all__ = [
    "MediawayError",
    "EncoderUnavailableError",
    "DeviceUnavailableError",
    "CaptureUnsupportedError",
    "InvalidStateError",
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


class DeviceUnavailableError(MediawayError):
    """A capture device or backend could not be opened.

    Maps the device ABI's NO_BACKEND / BACKEND_FAILURE / ACCESS_DENIED
    outcomes for video/audio capture opens.
    """


class CaptureUnsupportedError(MediawayError):
    """The ABI rejected this capture configuration as unsupported.

    Maps the device ABI's UNSUPPORTED outcome — today this is Screen capture
    from the C ABI (requires a GPU device handle whose C representation is
    deferred). Not a bug: a documented capability gap.
    """


class InvalidStateError(MediawayError):
    """A call violated a handle's typestate (e.g. add_track after begin()).

    Maps the container ABI's INVALID_STATE outcome. The wrappers make most of
    these unrepresentable, but defensive examples may still hit them.
    """
