//! Sans-IO MPEG-1/2/2.5 Layer III ("MP3") elementary stream mux and demux — no
//! OS I/O, no Mediaway types.
//!
//! v1 scope: Layer III only (see crate-local ADR-0001). Frames already-encoded
//! Layer III payloads with a correct 4-byte header — this crate does not encode
//! PCM into Layer III bitstreams (a codec's job, out of scope for a
//! container/framing crate).
//!
//! Like `adts-core`, MPEG audio frames have no container-level header — [`Muxer`]
//! appends one frame per call (no `finish()`); [`Demuxer`] is a true incremental
//! `push_bytes`/`poll_frame` reader.

#![forbid(unsafe_code)]

mod demux;
mod error;
mod mux;
mod types;

pub use demux::Demuxer;
pub use error::Error;
pub use mux::Muxer;
pub use types::{ChannelMode, FrameHeader, MpegVersion};
