//! `WebM` element IDs used by the v1 demux subset.
//!
//! See facade `ADR-0001` / this crate's `adr/0001`. Values are the raw EBML
//! element ID (marker bits included, per RFC 8794) — see
//! `docs/standards/registry.toml` (`rfc-8794-ebml`, `webm-container-guidelines`).

#![forbid(unsafe_code)]

/// `EBML` header — skipped whole in v1 demux (no `DocType` validation); the
/// mux side writes a minimal, spec-valid one (`adr/0003`).
pub const EBML_HEADER: u32 = 0x1A45_DFA3;
/// `EBML\EBMLVersion` (mux only — always `1`).
pub const EBML_VERSION: u32 = 0x4286;
/// `EBML\EBMLReadVersion` (mux only — always `1`).
pub const EBML_READ_VERSION: u32 = 0x42F7;
/// `EBML\EBMLMaxIDLength` (mux only — always `4`, matching [`decode_id`](crate::vint::decode_id)'s limit).
pub const EBML_MAX_ID_LENGTH: u32 = 0x42F2;
/// `EBML\EBMLMaxSizeLength` (mux only — always `8`).
pub const EBML_MAX_SIZE_LENGTH: u32 = 0x42F3;
/// `EBML\DocType` (mux only — ASCII `"webm"`).
pub const DOC_TYPE: u32 = 0x4282;
/// `EBML\DocTypeVersion` (mux only).
pub const DOC_TYPE_VERSION: u32 = 0x4287;
/// `EBML\DocTypeReadVersion` (mux only).
pub const DOC_TYPE_READ_VERSION: u32 = 0x4285;
/// `Segment` — top-level container; may be indefinite size.
pub const SEGMENT: u32 = 0x1853_8067;
/// `Segment\Info`.
pub const SEGMENT_INFO: u32 = 0x1549_A966;
/// `Segment\Info\TimecodeScale` (ns per tick; default `1_000_000`).
pub const TIMECODE_SCALE: u32 = 0x2A_D7B1;
/// `Segment\Tracks`.
pub const TRACKS: u32 = 0x1654_AE6B;
/// `Segment\Tracks\TrackEntry`.
pub const TRACK_ENTRY: u32 = 0xAE;
/// `TrackEntry\TrackNumber`.
pub const TRACK_NUMBER: u32 = 0xD7;
/// `TrackEntry\TrackType` (1 = video, 2 = audio, …).
pub const TRACK_TYPE: u32 = 0x83;
/// `TrackEntry\CodecID` (ASCII, e.g. `"V_VP9"`).
pub const CODEC_ID: u32 = 0x86;
/// `TrackEntry\Video`.
pub const VIDEO: u32 = 0xE0;
/// `Video\PixelWidth`.
pub const PIXEL_WIDTH: u32 = 0xB0;
/// `Video\PixelHeight`.
pub const PIXEL_HEIGHT: u32 = 0xBA;
/// `Segment\Cluster` — may be indefinite size.
pub const CLUSTER: u32 = 0x1F43_B675;
/// `Cluster\Timecode` (in `TimecodeScale` ticks, relative to `Segment` start).
pub const TIMECODE: u32 = 0xE7;
/// `Cluster\SimpleBlock`.
pub const SIMPLE_BLOCK: u32 = 0xA3;
/// `TrackEntry\Audio`.
pub const AUDIO: u32 = 0xE1;
/// `Audio\SamplingFrequency` (EBML Float; default `8000.0` Hz).
pub const SAMPLING_FREQUENCY: u32 = 0xB5;
/// `Audio\Channels` (default `1`).
pub const CHANNELS: u32 = 0x9F;
/// `Cluster\BlockGroup`.
pub const BLOCK_GROUP: u32 = 0xA0;
/// `BlockGroup\Block` — same wire format as `SimpleBlock`, but the keyframe
/// flag bit is reserved here; keyframe-ness is instead the *absence* of a
/// sibling `ReferenceBlock`.
pub const BLOCK: u32 = 0xA1;
/// `BlockGroup\BlockDuration` (in `TimecodeScale` ticks).
pub const BLOCK_DURATION: u32 = 0x9B;
/// `BlockGroup\ReferenceBlock` — presence (value is unused) marks the block as
/// not a keyframe.
pub const REFERENCE_BLOCK: u32 = 0xFB;
/// `Segment\Cues` — seek index; informational only (this crate does no
/// seeking itself, per sans-io — see crate-local ADR-0002).
pub const CUES: u32 = 0x1C53_BB6B;
/// `Cues\CuePoint`.
pub const CUE_POINT: u32 = 0xBB;
/// `CuePoint\CueTime`.
pub const CUE_TIME: u32 = 0xB3;
/// `CuePoint\CueTrackPositions`.
pub const CUE_TRACK_POSITIONS: u32 = 0xB7;
/// `CueTrackPositions\CueTrack`.
pub const CUE_TRACK: u32 = 0xF7;
/// `CueTrackPositions\CueClusterPosition` (byte offset from `Segment`'s data start).
pub const CUE_CLUSTER_POSITION: u32 = 0xF1;
/// `Segment\SeekHead` — informational only (see [`CUES`]).
pub const SEEK_HEAD: u32 = 0x114D_9B74;
/// `SeekHead\Seek`.
pub const SEEK: u32 = 0x4DBB;
/// `Seek\SeekID` (the referenced element's raw ID bytes).
pub const SEEK_ID: u32 = 0x53AB;
/// `Seek\SeekPosition` (byte offset from `Segment`'s data start).
pub const SEEK_POSITION: u32 = 0x53AC;

/// Master elements the walker descends into. Every other element ID
/// (recognized leaf or not) is treated as opaque and skipped by its own
/// element size.
#[must_use]
pub const fn is_descend_master(id: u32) -> bool {
    matches!(
        id,
        SEGMENT
            | SEGMENT_INFO
            | TRACKS
            | TRACK_ENTRY
            | VIDEO
            | AUDIO
            | CLUSTER
            | BLOCK_GROUP
            | CUES
            | CUE_POINT
            | CUE_TRACK_POSITIONS
            | SEEK_HEAD
            | SEEK
    )
}

/// IDs that only ever occur as a direct `Segment` child.
///
/// Seeing one of these while an indefinite-size `Cluster` is open is a
/// "sibling ID" — RFC 8794 §9.4 unknown-size resolution: the new element
/// implicitly closes the still-open `Cluster` before starting, rather than
/// nesting under it. Narrower than a full parent/child schema table (which
/// this crate doesn't model): this only recognizes the sibling shapes that
/// can actually follow a `Cluster` at `Segment` level, so an unmodeled
/// *real* child of `Cluster` (`SilentTracks`, `Void`, …) never gets mistaken
/// for a sibling.
#[must_use]
pub const fn is_segment_level_child(id: u32) -> bool {
    matches!(id, SEGMENT_INFO | TRACKS | CLUSTER | CUES | SEEK_HEAD)
}
/// `TrackEntry\CodecPrivate` (codec-specific config: `OpusHead` for Opus, the
/// VP9 uncompressed-header config for VP9, …).
pub const CODEC_PRIVATE: u32 = 0x63A2;
