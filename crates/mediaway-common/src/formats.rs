//! Pixel and sample format tags shared by encode/decode/device.

#![forbid(unsafe_code)]

/// Video pixel layout (planar / packed). Extend as backends need.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PixelFormat {
    /// 8-bit NV12 (YUV 4:2:0 semi-planar) — common HW encode input.
    Nv12,
    /// 8-bit I420 / YUV420P.
    I420,
    /// 8-bit BGRA packed.
    Bgra8,
    /// 8-bit RGBA packed.
    Rgba8,
    /// 8-bit YUYV / YUY2 packed (YUV 4:2:2, `Y0 U0 Y1 V0` byte order) — the
    /// most common raw format real V4L2 UVC webcams expose natively. Added
    /// for `mediaway-device-linux`'s V4L2 camera backend; see that crate's
    /// `adr/0002-v4l2-camera-capture.md`.
    Yuyv,
}

/// YUV sample range for [`PixelFormat::Nv12`] / [`PixelFormat::I420`] / [`PixelFormat::Yuyv`].
/// Irrelevant for packed RGB formats ([`PixelFormat::Bgra8`] / [`PixelFormat::Rgba8`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum ColorRange {
    /// "Legal"/broadcast range: 8-bit luma 16-235, chroma 16-240 — the common camera/H.264
    /// convention and this type's default.
    #[default]
    Video,
    /// Full range: 8-bit luma/chroma 0-255 — common for screen-capture/graphics-originated
    /// content.
    Full,
}

/// Audio PCM / sample layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SampleFormat {
    /// Signed 16-bit little-endian interleaved PCM.
    S16,
    /// Signed 32-bit little-endian interleaved PCM.
    S32,
    /// IEEE float32 interleaved PCM.
    F32,
}
