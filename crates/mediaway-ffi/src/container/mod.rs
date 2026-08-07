//! C ABI facade over [`mediaway_container`] (mux/demux).
//!
//! Design: `adr/0001-mp4-mux-demux-c-abi.md` — opaque handles (single `Box`, no
//! `Rc`/`Arc`), a 9-value `mediaway_status_t`, typestate (`Open`/`Live`) enforced at
//! runtime via a flat handle, `catch_unwind` panic safety with a per-handle `poisoned`
//! flag, and a hand-written header (`include/mediaway/container.h`). Design rules:
//! [`docs/spec/c-ffi.md`](../../../../docs/spec/c-ffi.md) (ADR-0004).
//!
//! This crate is the first `mediaway-*-ffi` crate in the workspace
//! ([`docs/spec/crate-packaging.md`](../../../../docs/spec/crate-packaging.md)). It exposes
//! `mediaway-container`'s mux/demux traits over a C ABI: opaque handles + integer error
//! codes, no panics/unwinding across the boundary.

#![allow(unsafe_code)] // FFI crate — see docs/conventions/code-style.md § unsafe

mod buffer;
mod status;
mod types;

#[cfg(feature = "demux")]
mod demuxer;
#[cfg(feature = "mux")]
mod muxer;

pub use status::MediawayStatus;
pub use types::{
    MediawayAudioTrackInfo, MediawayCodecKind, MediawayContainerFormat, MediawayPacket,
    MediawayPacketView, MediawayRational, MediawayStreamInfo, MediawayVideoTrackInfo,
};

#[cfg(feature = "mux")]
pub use buffer::mediaway_buffer_free;
#[cfg(feature = "demux")]
pub use buffer::{mediaway_packet_free, mediaway_stream_info_free};

#[cfg(feature = "demux")]
pub use demuxer::{
    DemuxerHandle, mediaway_demuxer_clear_decryption_key, mediaway_demuxer_close,
    mediaway_demuxer_create, mediaway_demuxer_create_for_format, mediaway_demuxer_poll_packet,
    mediaway_demuxer_push_bytes, mediaway_demuxer_set_decryption_key, mediaway_demuxer_stream_at,
    mediaway_demuxer_stream_count,
};
#[cfg(feature = "mux")]
pub use muxer::{
    MuxerHandle, mediaway_muxer_add_audio_track, mediaway_muxer_add_video_track,
    mediaway_muxer_begin, mediaway_muxer_close, mediaway_muxer_create,
    mediaway_muxer_create_for_format, mediaway_muxer_create_with_fragment_batch,
    mediaway_muxer_flush, mediaway_muxer_poll_bytes, mediaway_muxer_push_packet,
};

/// Runtime ABI version, matching `MEDIAWAY_CONTAINER_FFI_ABI_VERSION` in
/// `include/mediaway/container.h`.
///
/// A dynamically-loaded consumer (Python/Node/Go/...) that never compiles against the
/// header can call this to assert the loaded library matches what it was built against.
#[unsafe(no_mangle)]
pub const extern "C" fn mediaway_container_ffi_abi_version() -> u32 {
    0
}
