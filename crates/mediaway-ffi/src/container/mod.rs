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
mod adts_demuxer;
#[cfg(feature = "mux")]
mod adts_muxer;
#[cfg(feature = "demux")]
mod demuxer;
#[cfg(feature = "demux")]
mod flv_demuxer;
#[cfg(feature = "mux")]
mod flv_muxer;
#[cfg(feature = "demux")]
mod mp3_demuxer;
#[cfg(feature = "mux")]
mod mp3_muxer;
#[cfg(feature = "mux")]
mod muxer;
#[cfg(feature = "demux")]
mod ogg_demuxer;
#[cfg(feature = "mux")]
mod ogg_muxer;
#[cfg(feature = "demux")]
mod ts_demuxer;
#[cfg(feature = "mux")]
mod ts_muxer;

pub use status::MediawayStatus;
pub use types::{
    MediawayAudioTrackInfo, MediawayChannelMode, MediawayCodecKind, MediawayContainerFormat,
    MediawayMp3FrameHeader, MediawayMpegVersion, MediawayPacket, MediawayPacketView,
    MediawayRational, MediawayStreamInfo, MediawayTsElementaryStream, MediawayVideoTrackInfo,
};

#[cfg(feature = "mux")]
pub use buffer::mediaway_buffer_free;
#[cfg(feature = "demux")]
pub use buffer::{mediaway_packet_free, mediaway_stream_info_free};

#[cfg(feature = "demux")]
pub use adts_demuxer::{
    AdtsDemuxerHandle, mediaway_adts_demuxer_close, mediaway_adts_demuxer_create,
    mediaway_adts_demuxer_poll_packet, mediaway_adts_demuxer_push_bytes,
    mediaway_adts_demuxer_stream_at, mediaway_adts_demuxer_stream_count,
};
#[cfg(feature = "mux")]
pub use adts_muxer::{
    AdtsMuxerHandle, mediaway_adts_muxer_close, mediaway_adts_muxer_create,
    mediaway_adts_muxer_flush, mediaway_adts_muxer_poll_bytes, mediaway_adts_muxer_push_packet,
};
#[cfg(feature = "demux")]
pub use demuxer::{
    DemuxerHandle, mediaway_demuxer_clear_decryption_key, mediaway_demuxer_close,
    mediaway_demuxer_create, mediaway_demuxer_create_for_format, mediaway_demuxer_poll_packet,
    mediaway_demuxer_push_bytes, mediaway_demuxer_set_decryption_key, mediaway_demuxer_stream_at,
    mediaway_demuxer_stream_count,
};
#[cfg(feature = "demux")]
pub use flv_demuxer::{
    FlvDemuxerHandle, mediaway_flv_demuxer_close, mediaway_flv_demuxer_create,
    mediaway_flv_demuxer_poll_packet, mediaway_flv_demuxer_push_bytes,
    mediaway_flv_demuxer_stream_at, mediaway_flv_demuxer_stream_count,
};
#[cfg(feature = "mux")]
pub use flv_muxer::{
    FlvMuxerHandle, mediaway_flv_muxer_add_audio_track, mediaway_flv_muxer_add_video_track,
    mediaway_flv_muxer_close, mediaway_flv_muxer_create, mediaway_flv_muxer_push_packet,
    mediaway_flv_muxer_write_header,
};
#[cfg(feature = "demux")]
pub use mp3_demuxer::{
    Mp3DemuxerHandle, mediaway_mp3_demuxer_close, mediaway_mp3_demuxer_create,
    mediaway_mp3_demuxer_poll_packet, mediaway_mp3_demuxer_push_bytes,
    mediaway_mp3_demuxer_stream_at, mediaway_mp3_demuxer_stream_count,
};
#[cfg(feature = "mux")]
pub use mp3_muxer::{
    Mp3MuxerHandle, mediaway_mp3_muxer_close, mediaway_mp3_muxer_create,
    mediaway_mp3_muxer_write_frame,
};
#[cfg(feature = "mux")]
pub use muxer::{
    MuxerHandle, mediaway_muxer_add_audio_track, mediaway_muxer_add_video_track,
    mediaway_muxer_begin, mediaway_muxer_close, mediaway_muxer_create,
    mediaway_muxer_create_for_format, mediaway_muxer_create_with_fragment_batch,
    mediaway_muxer_flush, mediaway_muxer_poll_bytes, mediaway_muxer_push_packet,
};
#[cfg(feature = "demux")]
pub use ogg_demuxer::{
    OggDemuxerHandle, mediaway_ogg_demuxer_close, mediaway_ogg_demuxer_create,
    mediaway_ogg_demuxer_poll_packet, mediaway_ogg_demuxer_push_bytes,
    mediaway_ogg_demuxer_stream_at, mediaway_ogg_demuxer_stream_count,
};
#[cfg(feature = "mux")]
pub use ogg_muxer::{
    OggMuxerHandle, mediaway_ogg_muxer_close, mediaway_ogg_muxer_create, mediaway_ogg_muxer_flush,
    mediaway_ogg_muxer_poll_bytes, mediaway_ogg_muxer_push_packet,
};
#[cfg(feature = "demux")]
pub use ts_demuxer::{
    TsDemuxerHandle, mediaway_ts_demuxer_close, mediaway_ts_demuxer_create,
    mediaway_ts_demuxer_finish, mediaway_ts_demuxer_finish_free, mediaway_ts_demuxer_poll_packet,
    mediaway_ts_demuxer_push_bytes, mediaway_ts_demuxer_stream_at,
    mediaway_ts_demuxer_stream_count,
};
#[cfg(feature = "mux")]
pub use ts_muxer::{
    TsMuxerHandle, mediaway_ts_muxer_close, mediaway_ts_muxer_create,
    mediaway_ts_muxer_write_access_unit, mediaway_ts_muxer_write_pat_pmt,
};

/// Runtime ABI version, matching `MEDIAWAY_CONTAINER_FFI_ABI_VERSION` in
/// `include/mediaway/container.h`.
///
/// A dynamically-loaded consumer (Python/Node/Go/...) that never compiles against the
/// header can call this to assert the loaded library matches what it was built against.
///
/// Was hardcoded `0` through ABI v1 (`WebM`'s `mediaway_muxer_create_for_format` addition) —
/// a drift bug, since the header macro had already moved to `1` and nothing kept this
/// literal in sync. Fixed at `2` (Ogg, `adr/0004-ogg-adts-c-abi.md`), then `3` (ADTS, same
/// pass), then `4` (FLV, `adr/0005-flv-c-abi.md`), then `5` (MPEG-TS,
/// `adr/0006-mpeg-ts-c-abi.md`), then `6` (MP3's dedicated `mediaway_mp3_muxer_t`/
/// `mediaway_mp3_demuxer_t` handles, `adr/0007-mp3-c-abi.md`).
#[unsafe(no_mangle)]
pub const extern "C" fn mediaway_container_ffi_abi_version() -> u32 {
    6
}
