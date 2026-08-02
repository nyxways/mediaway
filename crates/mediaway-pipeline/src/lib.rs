//! Convenience **pipeline** layer — composes [`mediaway_encoder`] +
//! [`mediaway_container`] (+ [`mediaway_device`] for capture) so apps don't
//! hand-roll the encoder→muxer poll loop. See
//! [ADR-0014](../../../docs/adr/0014-pipeline-convenience-crate.md).
//!
//! - **Low-level stays reachable.** [`mediaway_encoder::VideoEncoder`] and
//!   [`mediaway_container::mp4::Muxer`] are unchanged and fully usable without
//!   this crate — see `examples/mux_roundtrip.rs` in the workspace root. This
//!   crate only adds a thin composition on top; it is not the only way in.
//! - [`EncodeSession`] — encoder + single-track MP4 muxer composition.
//! - [`FrameFilter`] — optional mid-pipeline frame transform chain on
//!   [`EncodeSession`] (see [ADR-0001](../../adr/0001-frame-filter-hook.md)).
//! - [`platform`] — OS auto-dispatch (moved from the former `examples/platform.rs`).

#![forbid(unsafe_code)]

mod error;
mod filter;
pub mod platform;
mod session;

pub use error::PipelineError;
pub use filter::{FilterError, FrameFilter};
pub use session::EncodeSession;
