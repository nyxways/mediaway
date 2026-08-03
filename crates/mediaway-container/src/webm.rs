//! Mediaway-typed `WebM` mux + demux surface over [`ebml_webm`].
//!
//! `ebml_webm::TrackInfo::codec_id` is a raw `WebM` string (e.g. `"V_VP9"`,
//! `"V_VP8"`); [`Demuxer::streams`] maps only the codecs [`CodecKind`]
//! already has (`Vp9`, `Av1`, `Opus`, `Aac`, `Vorbis` — `CodecKind::Vorbis`
//! closed this gap 2026-07-29), and [`Muxer::add_track`] rejects any other
//! codec rather than writing a `CodecID` this facade can't round-trip. VP8
//! tracks are still **omitted** on demux (no `CodecKind::Vp8` yet) and their
//! frames are dropped in [`Demuxer::poll_packet`] — see `ebml-webm/adr/0001`
//! and this crate's `adr/0001` for the remaining tracked gap. Mux: see
//! `adr/0003` (this crate) and `ebml-webm/adr/0003`.

#![forbid(unsafe_code)]

#[cfg(feature = "demux")]
use crate::Demux;
#[cfg(feature = "mux")]
use crate::Mux;
#[cfg(feature = "demux")]
use ebml_webm::Demuxer as CoreDemuxer;
use ebml_webm::TrackInfo as CoreTrackInfo;
pub use ebml_webm::{CuePoint, SeekEntry};
#[cfg(feature = "mux")]
use ebml_webm::{Muxer as CoreMuxer, mux::Live as CoreLive, mux::Open as CoreOpen};
use mediaway_common::{
    Bytes, CodecKind, Packet, Rational as MwRational, StreamInfo, VideoGeometry,
};

/// Demuxer wrapping [`ebml_webm::Demuxer`] with a Mediaway stream cache.
#[cfg(feature = "demux")]
#[derive(Debug, Default)]
pub struct Demuxer {
    inner: CoreDemuxer,
    streams: Vec<StreamInfo>,
}

#[cfg(feature = "demux")]
impl Demuxer {
    /// Empty demuxer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed bytes.
    pub fn push_bytes(&mut self, chunk: &[u8]) {
        self.inner.push_bytes(chunk);
        self.sync_streams();
    }

    /// Tracks whose `WebM` `CodecID` maps to a Mediaway [`CodecKind`] (see
    /// module docs for the current mapping gap).
    #[must_use]
    pub fn streams(&self) -> &[StreamInfo] {
        &self.streams
    }

    /// `Segment\Cues\CuePoint` entries seen so far — informational seek
    /// index; this crate does no seeking itself (sans-io: I/O and
    /// seek-driven re-reads are the adapter's job). Passed through from
    /// [`ebml_webm::Demuxer::cues`] unchanged (plain offsets, nothing
    /// codec-specific to convert).
    #[must_use]
    pub fn cues(&self) -> &[CuePoint] {
        self.inner.cues()
    }

    /// `Segment\SeekHead\Seek` entries seen so far — same "informational
    /// only" status as [`cues`](Self::cues).
    #[must_use]
    pub fn seek_head(&self) -> &[SeekEntry] {
        self.inner.seek_head()
    }

    /// Next demuxed packet. Frames from tracks with no `CodecKind` mapping
    /// are skipped (not returned as `Err` — the `Demux` trait has no error
    /// channel here; see module docs).
    pub fn poll_packet(&mut self) -> Option<Packet> {
        loop {
            let frame = self.inner.poll_frame()?;
            let id = track_id(frame.track_number);
            if !self.streams.iter().any(|s| s.id() == id) {
                continue;
            }
            return Some(Packet {
                stream_id: id,
                pts: frame.timecode,
                // `WebM` `SimpleBlock` carries no separate decode timestamp;
                // v1 has no B-frame reorder support, so pts == dts.
                dts: frame.timecode,
                // `BlockGroup`'s `BlockDuration` is in the same tick unit as
                // `timecode` (both scaled by `TimecodeScale`); `SimpleBlock`
                // frames carry no duration, so default to 0.
                duration: frame.duration_ticks.unwrap_or(0),
                is_keyframe: frame.is_keyframe,
                is_discard: false,
                payload: frame.payload,
            });
        }
    }

    fn sync_streams(&mut self) {
        let tb = self.inner.time_base();
        let time_base = MwRational::new(tb.num, tb.den);
        self.streams = self
            .inner
            .streams()
            .iter()
            .filter_map(|t| to_stream_info(t, time_base))
            .collect();
    }
}

#[cfg(feature = "demux")]
#[allow(clippy::use_self)]
impl Demux for Demuxer {
    fn push_bytes(&mut self, chunk: &[u8]) {
        Demuxer::push_bytes(self, chunk);
    }

    fn streams(&self) -> &[StreamInfo] {
        Demuxer::streams(self)
    }

    fn poll_packet(&mut self) -> Option<Packet> {
        Demuxer::poll_packet(self)
    }
}

/// `WebM` `TrackNumber` <-> [`StreamInfo`]/[`Packet`]'s `u32` id — a 1:1,
/// lossless mapping for every `track_number` either side of this facade
/// itself ever produces (mux assigns small sequential numbers; demux
/// saturates an out-of-`u32`-range `TrackNumber` to `u32::MAX`, a real `WebM`
/// file this facade round-trips would never hit in practice).
fn track_id(track_number: u64) -> u32 {
    u32::try_from(track_number).unwrap_or(u32::MAX)
}

#[cfg(feature = "demux")]
fn to_stream_info(t: &CoreTrackInfo, time_base: MwRational) -> Option<StreamInfo> {
    let codec = codec_kind(&t.codec_id)?;
    let id = track_id(t.track_number);
    if t.is_video() {
        Some(StreamInfo::Video {
            id,
            codec,
            time_base,
            geometry: VideoGeometry {
                width: t.width,
                height: t.height,
            },
            extra_data: t.codec_private.clone().unwrap_or_default(),
        })
    } else {
        Some(StreamInfo::Audio {
            id,
            codec,
            time_base,
            extra_data: Bytes::new(),
            sample_rate: sample_rate_u32(t.sample_rate),
            channels: channels_u16(t.channels),
        })
    }
}

/// `ebml_webm::TrackInfo::sample_rate` is an EBML Float (`f64`); MP4/StreamInfo
/// use `u32` Hz. Clamps in-range finite values rather than wrapping; NaN and
/// +-infinity (malformed input either way) map to `0` ("unknown"), not a
/// saturated bound.
#[cfg(feature = "demux")]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "saturating conversion from EBML Float sample rate to Hz"
)]
fn sample_rate_u32(sample_rate: f64) -> u32 {
    if sample_rate.is_finite() {
        sample_rate.clamp(0.0, f64::from(u32::MAX)) as u32
    } else {
        0
    }
}

#[cfg(feature = "demux")]
#[allow(
    clippy::cast_possible_truncation,
    reason = "saturating conversion from EBML uint channel count to StreamInfo's u16"
)]
const fn channels_u16(channels: u32) -> u16 {
    if channels > 0xFFFF {
        u16::MAX
    } else {
        channels as u16
    }
}

/// Map a `WebM` `CodecID` string to [`CodecKind`]. `None` for codecs
/// `CodecKind` has no variant for yet (notably VP8, Vorbis).
#[cfg(feature = "demux")]
fn codec_kind(codec_id: &str) -> Option<CodecKind> {
    match codec_id {
        "V_VP9" => Some(CodecKind::Vp9),
        "V_AV1" => Some(CodecKind::Av1),
        "A_OPUS" => Some(CodecKind::Opus),
        "A_VORBIS" => Some(CodecKind::Vorbis),
        id if id.starts_with("A_AAC") => Some(CodecKind::Aac),
        _ => None,
    }
}

/// Map a [`CodecKind`] to the `WebM` `CodecID` string [`Muxer::add_track`]
/// writes — the exact reverse of [`codec_kind`] (kept as two separate small
/// functions rather than one bidirectional table: the sets aren't quite
/// symmetric, e.g. demux's `starts_with("A_AAC")` accepts profile-suffixed
/// IDs a real encoder never needs to write). `None` for any [`CodecKind`]
/// `WebM` doesn't natively carry (H.264, HEVC, MP3, …) — [`Muxer::add_track`]
/// turns that into [`Error::UnsupportedCodec`].
#[cfg(feature = "mux")]
const fn webm_codec_id(codec: CodecKind) -> Option<&'static str> {
    match codec {
        CodecKind::Vp9 => Some("V_VP9"),
        CodecKind::Av1 => Some("V_AV1"),
        CodecKind::Opus => Some("A_OPUS"),
        CodecKind::Vorbis => Some("A_VORBIS"),
        CodecKind::Aac => Some("A_AAC"),
        _ => None,
    }
}

/// [`Muxer`] error.
#[cfg(feature = "mux")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Wraps [`ebml_webm::MuxError`] (bad `TrackNumber`, unregistered track).
    #[error(transparent)]
    Mux(#[from] ebml_webm::MuxError),
    /// [`Muxer::add_track`]: `WebM` has no `CodecID` for this [`CodecKind`]
    /// (see [`webm_codec_id`]).
    #[error("WebM mux has no CodecID mapping for {0:?}")]
    UnsupportedCodec(CodecKind),
    /// A `Mux::push_packet` call with no track registered for `stream_id`.
    #[error("push_packet references unregistered stream_id {0}")]
    UnknownStream(u32),
}

/// Track registration phase — call [`Muxer::begin`] before pushing packets.
#[cfg(feature = "mux")]
#[derive(Debug, Clone, Copy, Default)]
pub struct Open;

/// Streaming phase — `push_packet`/`poll_bytes`.
#[cfg(feature = "mux")]
#[derive(Debug, Clone, Copy, Default)]
pub struct Live;

/// Muxer wrapping [`ebml_webm::Muxer`] — typestate mirrors [`mp4::Muxer`](crate::mp4::Muxer).
#[cfg(feature = "mux")]
#[derive(Debug)]
pub struct Muxer<S = Open> {
    open: Option<CoreMuxer<CoreOpen>>,
    live: Option<CoreMuxer<CoreLive>>,
    tracks_cache: Vec<StreamInfo>,
    _state: core::marker::PhantomData<S>,
}

#[cfg(feature = "mux")]
impl Muxer<Open> {
    /// Empty muxer.
    #[must_use]
    pub fn new() -> Self {
        Self {
            open: Some(CoreMuxer::new()),
            live: None,
            tracks_cache: Vec::new(),
            _state: core::marker::PhantomData,
        }
    }

    /// Registered tracks so far.
    #[must_use]
    pub fn tracks(&self) -> &[StreamInfo] {
        &self.tracks_cache
    }

    /// Register a track.
    ///
    /// # Errors
    ///
    /// Register a track (video or audio) for muxing, returning its track
    /// number. The stream's `extra_data` (e.g. `OpusHead`) becomes the
    /// `CodecPrivate` element of the track entry.
    ///
    /// # Errors
    ///
    /// [`Error::UnsupportedCodec`] if `WebM` has no `CodecID` for this
    /// track's codec; [`Error::Mux`] for an invalid/duplicate track number.
    pub fn add_track(&mut self, track: StreamInfo) -> Result<u32, Error> {
        /// `CodecPrivate` payload for a track (`OpusHead` etc.), when the
        /// caller supplied one via `extra_data`.
        fn track_extra(track: &StreamInfo) -> Option<Bytes> {
            match track {
                StreamInfo::Video { extra_data, .. } | StreamInfo::Audio { extra_data, .. }
                    if !extra_data.is_empty() =>
                {
                    Some(extra_data.clone())
                }
                _ => None,
            }
        }
        let codec_id =
            webm_codec_id(track.codec()).ok_or_else(|| Error::UnsupportedCodec(track.codec()))?;
        let id = track.id();
        let core_track = match &track {
            StreamInfo::Video {
                geometry,
                extra_data,
                ..
            } => CoreTrackInfo {
                track_number: u64::from(id),
                track_type: 1,
                codec_id: codec_id.to_string(),
                codec_private: (!extra_data.is_empty()).then(|| extra_data.clone()),
                width: geometry.width,
                height: geometry.height,
                sample_rate: 8000.0,
                channels: 1,
            },
            StreamInfo::Audio {
                sample_rate,
                channels,
                ..
            } => CoreTrackInfo {
                track_number: u64::from(id),
                track_type: 2,
                codec_id: codec_id.to_string(),
                codec_private: track_extra(&track),
                width: 0,
                height: 0,
                sample_rate: f64::from(*sample_rate),
                channels: u32::from(*channels),
            },
            // `StreamInfo` is `#[non_exhaustive]` — a future non-Video/Audio
            // variant has no WebM track shape to map to yet.
            _ => return Err(Error::UnsupportedCodec(track.codec())),
        };
        let open = self.open.as_mut().ok_or(Error::UnknownStream(id))?;
        open.add_track(core_track)?;
        self.tracks_cache.push(track);
        Ok(id)
    }

    /// Lock tracks and enter the live streaming state.
    #[must_use]
    pub fn begin(mut self) -> Muxer<Live> {
        let open = self.open.take().unwrap_or_default();
        Muxer {
            open: None,
            live: Some(open.begin()),
            tracks_cache: self.tracks_cache,
            _state: core::marker::PhantomData,
        }
    }
}

#[cfg(feature = "mux")]
impl Default for Muxer<Open> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "mux")]
impl Muxer<Live> {
    /// Registered tracks.
    #[must_use]
    pub fn tracks(&self) -> &[StreamInfo] {
        &self.tracks_cache
    }

    /// Push one packet.
    ///
    /// # Errors
    ///
    /// [`Error::UnknownStream`] if `packet.stream_id` was never registered
    /// via `add_track`; [`Error::Mux`] on an underlying mux error.
    pub fn push_packet(&mut self, packet: &Packet) -> Result<(), Error> {
        if !self.tracks_cache.iter().any(|t| t.id() == packet.stream_id) {
            return Err(Error::UnknownStream(packet.stream_id));
        }
        let live = self
            .live
            .as_mut()
            .ok_or(Error::UnknownStream(packet.stream_id))?;
        live.push_frame(
            u64::from(packet.stream_id),
            packet.pts,
            packet.is_keyframe,
            &packet.payload,
        )?;
        Ok(())
    }

    /// Force the open `Cluster` to close.
    pub fn flush(&mut self) {
        if let Some(live) = self.live.as_mut() {
            live.flush();
        }
    }

    /// Poll container bytes.
    pub fn poll_bytes(&mut self, out: &mut Vec<u8>) -> usize {
        self.live.as_mut().map_or(0, |m| m.poll_bytes(out))
    }
}

#[cfg(feature = "mux")]
#[allow(clippy::use_self)]
impl Mux for Muxer<Live> {
    type Error = Error;

    fn push_packet(&mut self, packet: &Packet) -> Result<(), Self::Error> {
        Muxer::<Live>::push_packet(self, packet)
    }

    fn flush(&mut self) {
        Muxer::<Live>::flush(self);
    }

    fn poll_bytes(&mut self, out: &mut Vec<u8>) -> usize {
        Muxer::<Live>::poll_bytes(self, out)
    }
}

#[cfg(test)]
#[path = "webm_tests.rs"]
mod tests;
