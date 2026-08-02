//! Sans-IO FLV (Flash Video) tag mux and demux — no OS I/O, no Mediaway types.
//!
//! Frames FLV's file header + tag/`PreviousTagSize` structure only — it does not
//! interpret or build the codec-specific sub-framing inside a tag's data
//! (`AudioTagHeader`/`VideoTagHeader`, e.g. `AVCPacketType`/composition time),
//! the same "frame, don't encode" boundary as this workspace's
//! `adts`/`mpeg-audio`/`ogg` crates.
//!
//! [`Muxer`] has no `finish()` — FLV tags are independently appendable, each
//! self-trailed with its own `PreviousTagSize`. [`Demuxer`] is a true incremental
//! `push_bytes`/`poll_tag` reader.

#![forbid(unsafe_code)]

mod demux;
mod error;
mod mux;
mod types;

pub use demux::Demuxer;
pub use error::Error;
pub use mux::Muxer;
pub use types::{Tag, TagType};
