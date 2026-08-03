//! [`FrameFilter`] — mid-pipeline frame transform hook on [`crate::EncodeSession`].
//!
//! See [ADR-0001](../../adr/0001-frame-filter-hook.md) for the full design.

#![forbid(unsafe_code)]

use mediaway_common::VideoFrame;
use thiserror::Error;

/// One step of an [`crate::EncodeSession`] frame filter chain.
///
/// Operates on `VideoFrameStorage::Cpu` frames only (see ADR-0001) — a session
/// with a non-empty filter chain rejects `Gpu`-backed frames with
/// [`FilterError::GpuFrameUnsupported`] rather than silently reading them back.
pub trait FrameFilter: 'static {
    /// Transform one frame. May return a different frame (new pixels, new pts)
    /// or the input unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`FilterError`] when the filter rejects or fails to process `frame`.
    fn process(&mut self, frame: VideoFrame) -> Result<VideoFrame, FilterError>;
}

/// Error from a [`FrameFilter`] chain step.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FilterError {
    /// A filter in the chain rejected or failed to process the frame.
    /// Details in logs when available (mirrors `EncodeError::Backend`).
    #[error("frame filter failed")]
    Rejected,
    /// A filter chain is configured but this frame is GPU-backed
    /// (`VideoFrameStorage::Gpu`) — v1 filters are CPU-frame-only (ADR-0001).
    #[error("frame filter chain does not support GPU-backed frames")]
    GpuFrameUnsupported,
}
