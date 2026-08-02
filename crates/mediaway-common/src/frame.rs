//! Uncompressed video/audio frames for encode input and decode output.

#![forbid(unsafe_code)]

use crate::formats::{PixelFormat, SampleFormat};
use crate::gpu::GpuBufferHandle;
use bytes::Bytes;

/// One video frame — CPU planes or a GPU handle (Zero-Copy).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoFrame {
    /// Presentation timestamp in the stream timebase.
    pub pts: i64,
    /// Duration in timebase units (`0` if unknown).
    pub duration: u64,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Pixel layout.
    pub format: PixelFormat,
    /// Backing store.
    pub storage: VideoFrameStorage,
}

/// Where pixel data lives.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum VideoFrameStorage {
    /// CPU-accessible tightly packed or planar bytes (may imply copy into HW).
    ///
    /// Prefer [`VideoFrameStorage::Gpu`] on hot encode paths. Using CPU storage
    /// with a HW encoder typically requires upload — backends must document that.
    Cpu {
        /// Plane bytes (layout implied by [`PixelFormat`]).
        data: Bytes,
    },
    /// GPU texture / surface — Zero-Copy when the backend accepts the variant.
    Gpu(GpuBufferHandle),
}

/// One audio buffer (interleaved PCM).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioFrame {
    /// Presentation timestamp in the stream timebase.
    pub pts: i64,
    /// Duration in timebase units.
    pub duration: u64,
    /// Sample rate (Hz).
    pub sample_rate: u32,
    /// Channel count.
    pub channels: u16,
    /// PCM sample format.
    pub format: SampleFormat,
    /// Interleaved sample bytes.
    pub data: Bytes,
}
