//! Mediaway-typed FLV (Flash Video) mux + demux over [`flv_core`].
//!
//! [`flv_core`] frames tags only — it does not interpret the codec-specific
//! `AudioTagHeader`/`VideoTagHeader` sub-byte(s) inside a tag's `data`. This
//! module reads/builds those bytes on both sides (the same "read/build a
//! codec's declared sub-header, don't decode audio/video" boundary
//! `iso-bmff` already crosses for `esds`/AVCC): [`Demuxer`] splits
//! sequence-header tags (`extra_data`) from data tags (`Packet`s);
//! [`Muxer`] does the inverse — given a registered track's `extra_data`, it
//! writes the sequence-header tag once before the first data tag.
//!
//! ## Codec coverage
//!
//! Only `AVC` video (`VideoTagHeader.CodecID == 7`) and `AAC`/`MP3` audio
//! (`AudioTagHeader.SoundFormat == 10 | 2`) are recognized — the common,
//! still-in-use real-world case. Other codecs (VP6, Sorenson H.263, Screen
//! Video, Nellymoser, …) have no [`CodecKind`] mapping: [`Demuxer`] **drops**
//! their tags (same posture as `mediaway-container::webm`'s VP8/Vorbis gap,
//! not silently mismuxed as something else) and [`Muxer::add_track`]
//! **errors** rather than writing an unrecognized tag shape. "Enhanced
//! RTMP"/"Enhanced FLV" (2023, `ExVideoTagHeader`, HEVC/AV1/VP9 support) is
//! out of scope — this module targets the original FLV spec's tag shape.
//!
//! All tags share one millisecond timebase (`Tag::timestamp_ms`) —
//! `time_base = 1 / 1000` for both the video and audio stream.
//!
//! [`Muxer`] does not implement [`crate::Mux`] (see `adr/0002` in this
//! crate): a [`Packet`] alone carries no codec, so a track's codec/`extra_data`
//! must be registered separately via [`Muxer::add_track`] before
//! [`Muxer::push_packet`] can build the right `AudioTagHeader`/`VideoTagHeader`.

#![forbid(unsafe_code)]

use crate::Demux;
use flv_core::{Demuxer as CoreDemuxer, Muxer as CoreMuxer, Tag, TagType};
use mediaway_common::{Bytes, CodecKind, Packet, Rational, StreamInfo};

/// FLV facade mux/demux error.
///
/// Wraps [`flv_core::Error`] (tag framing) plus the track-registration/codec-
/// mapping errors specific to this codec-aware facade — the [`flv_core`] core
/// itself has no concept of "track" or "codec" (see module docs).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Tag framing error from the underlying [`flv_core`] core (bad signature,
    /// oversized tag data, a tag written before the file header, ...).
    #[error(transparent)]
    Tag(#[from] flv_core::Error),
    /// [`Muxer::add_track`] was given a codec with no FLV tag-header mapping
    /// (see module docs on codec coverage).
    #[error("FLV mux has no tag encoding for codec {0:?}")]
    UnsupportedCodec(CodecKind),
    /// [`Muxer::push_packet`] referenced a `stream_id` with no matching
    /// [`Muxer::add_track`] call.
    #[error("push_packet: no track registered for stream_id {0}")]
    UnregisteredStream(u32),
}

const VIDEO_STREAM_ID: u32 = 0;
const AUDIO_STREAM_ID: u32 = 1;
const MS_TIME_BASE: Rational = Rational::new(1, 1_000);

/// Registered mux track: recognized codec + whether the sequence-header tag
/// has been written yet (MP3 audio has none — see [`Muxer::push_packet`]).
#[derive(Debug)]
struct MuxTrack {
    codec: CodecKind,
    extra_data: Bytes,
    header_written: bool,
}

/// Live mux session (see module docs on why this has its own method shape
/// instead of [`crate::Mux`]).
#[derive(Debug, Default)]
pub struct Muxer {
    inner: CoreMuxer,
    video: Option<MuxTrack>,
    audio: Option<MuxTrack>,
}

impl Muxer {
    /// New mux session (call [`Self::write_header`] before any tag).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Write the FLV file header, declaring whether audio/video tags follow.
    pub fn write_header(&mut self, has_audio: bool, has_video: bool, out: &mut Vec<u8>) {
        self.inner.write_header(has_audio, has_video, out);
    }

    /// Register a track ahead of [`Self::push_packet`]. FLV has exactly one
    /// video and one audio slot (no track-id field in the format itself) —
    /// `stream`'s own `id` is ignored; video and audio tracks are
    /// distinguished by [`StreamInfo`] variant, matching [`Demuxer`]'s fixed
    /// `VIDEO_STREAM_ID`/`AUDIO_STREAM_ID` scheme. `stream`'s
    /// `extra_data` becomes the sequence-header tag (AVCC / AAC
    /// `AudioSpecificConfig`) written once before the first data tag on that
    /// track; MP3 audio has no sequence header and none is written.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedCodec`] for a codec this facade cannot
    /// encode as an FLV tag (see module docs on codec coverage).
    pub fn add_track(&mut self, stream: &StreamInfo) -> Result<(), Error> {
        match stream {
            StreamInfo::Video {
                codec, extra_data, ..
            } => {
                if *codec != CodecKind::H264 {
                    return Err(Error::UnsupportedCodec(*codec));
                }
                self.video = Some(MuxTrack {
                    codec: *codec,
                    extra_data: extra_data.clone(), // clone: track state must outlive the borrowed `StreamInfo`
                    header_written: false,
                });
            }
            StreamInfo::Audio {
                codec, extra_data, ..
            } => {
                if !matches!(codec, CodecKind::Aac | CodecKind::Mp3) {
                    return Err(Error::UnsupportedCodec(*codec));
                }
                self.audio = Some(MuxTrack {
                    codec: *codec,
                    extra_data: extra_data.clone(), // clone: track state must outlive the borrowed `StreamInfo`
                    header_written: false,
                });
            }
            // `StreamInfo` is `#[non_exhaustive]` (mediaway_common may add
            // variants beyond `Video`/`Audio`) — no FLV tag shape exists for
            // anything else today.
            other => return Err(Error::UnsupportedCodec(other.codec())),
        }
        Ok(())
    }

    /// Mux one packet: writes the track's sequence-header tag first (once,
    /// only for codecs that have one) then the data tag.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnregisteredStream`] if `packet.stream_id` isn't
    /// `VIDEO_STREAM_ID`/`AUDIO_STREAM_ID` or has no matching
    /// [`Self::add_track`] call, or propagates a tag framing error
    /// ([`Error::Tag`]).
    pub fn push_packet(&mut self, packet: &Packet, out: &mut Vec<u8>) -> Result<(), Error> {
        match packet.stream_id {
            VIDEO_STREAM_ID => self.push_video(packet, out),
            AUDIO_STREAM_ID => self.push_audio(packet, out),
            other => Err(Error::UnregisteredStream(other)),
        }
    }

    fn push_video(&mut self, packet: &Packet, out: &mut Vec<u8>) -> Result<(), Error> {
        let Some(track) = self.video.as_ref() else {
            return Err(Error::UnregisteredStream(packet.stream_id));
        };
        if !track.header_written {
            let data = avc_seq_header_data(&track.extra_data);
            self.inner.write_tag(
                &Tag {
                    tag_type: TagType::Video,
                    timestamp_ms: clamp_timestamp(packet.dts),
                    data,
                },
                out,
            )?;
            if let Some(track) = self.video.as_mut() {
                track.header_written = true;
            }
        }
        self.inner.write_tag(
            &Tag {
                tag_type: TagType::Video,
                timestamp_ms: clamp_timestamp(packet.dts),
                data: avc_nalu_data(packet),
            },
            out,
        )?;
        Ok(())
    }

    fn push_audio(&mut self, packet: &Packet, out: &mut Vec<u8>) -> Result<(), Error> {
        let Some(track) = self.audio.as_ref() else {
            return Err(Error::UnregisteredStream(packet.stream_id));
        };
        match track.codec {
            CodecKind::Aac => {
                if !track.header_written {
                    let data = aac_seq_header_data(&track.extra_data);
                    self.inner.write_tag(
                        &Tag {
                            tag_type: TagType::Audio,
                            timestamp_ms: clamp_timestamp(packet.dts),
                            data,
                        },
                        out,
                    )?;
                    if let Some(track) = self.audio.as_mut() {
                        track.header_written = true;
                    }
                }
                self.inner.write_tag(
                    &Tag {
                        tag_type: TagType::Audio,
                        timestamp_ms: clamp_timestamp(packet.dts),
                        data: aac_raw_data(&packet.payload),
                    },
                    out,
                )?;
            }
            CodecKind::Mp3 => {
                self.inner.write_tag(
                    &Tag {
                        tag_type: TagType::Audio,
                        timestamp_ms: clamp_timestamp(packet.dts),
                        data: mp3_data(&packet.payload),
                    },
                    out,
                )?;
            }
            // `add_track` rejects every other codec — kept as a returned
            // error (not `unreachable!()`) since panics are forbidden
            // outside tests.
            other => return Err(Error::UnsupportedCodec(other)),
        }
        Ok(())
    }

    /// Append one raw tag (already codec-sub-framed by the caller — bypasses
    /// track state, e.g. for `ScriptData`/AMF metadata tags this facade
    /// otherwise has no support for).
    ///
    /// # Errors
    ///
    /// Returns [`Error::Tag`] wrapping [`flv_core::Error::HeaderNotWritten`] if
    /// called before [`Self::write_header`], or [`flv_core::Error::TagDataTooLarge`].
    pub fn write_tag(&self, tag: &Tag, out: &mut Vec<u8>) -> Result<(), Error> {
        self.inner.write_tag(tag, out)?;
        Ok(())
    }
}

/// `VideoTagHeader` + payload for the AVC sequence-header tag
/// (`FrameType=1` key, `CodecID=7` AVC, `AVCPacketType=0`,
/// `CompositionTime=0`) wrapping the `AVCDecoderConfigurationRecord` `avcc`.
fn avc_seq_header_data(avcc: &Bytes) -> Bytes {
    let mut data = Vec::with_capacity(5 + avcc.len());
    data.push(0x17); // FrameType=1(key) << 4 | CodecID=7(AVC)
    data.push(0); // AVCPacketType = 0 (sequence header)
    data.extend_from_slice(&[0, 0, 0]); // CompositionTime = 0 (not applicable)
    data.extend_from_slice(avcc);
    Bytes::from(data)
}

/// `VideoTagHeader` + payload for one AVC NALU data tag (`AVCPacketType=1`),
/// signing `packet.pts - packet.dts` into the 24-bit `CompositionTime` field.
#[allow(
    clippy::cast_possible_truncation,
    reason = "only the low 24 bits of composition_time are ever written, matching the field width"
)]
fn avc_nalu_data(packet: &Packet) -> Bytes {
    let frame_type: u8 = if packet.is_keyframe { 1 } else { 2 };
    let mut data = Vec::with_capacity(5 + packet.payload.len());
    data.push((frame_type << 4) | 7);
    data.push(1); // AVCPacketType = 1 (NALU)
    let ct = composition_time(packet).to_be_bytes();
    data.extend_from_slice(&ct[1..4]);
    data.extend_from_slice(&packet.payload);
    Bytes::from(data)
}

fn composition_time(packet: &Packet) -> i32 {
    let diff = packet.pts.saturating_sub(packet.dts);
    i32::try_from(diff).unwrap_or_else(|_| {
        if diff.is_positive() {
            i32::MAX
        } else {
            i32::MIN
        }
    })
}

fn clamp_timestamp(ts: i64) -> u32 {
    u32::try_from(ts).unwrap_or(0)
}

/// `AudioTagHeader` + payload for the AAC sequence-header tag
/// (`SoundFormat=10`, `AACPacketType=0`) wrapping the `AudioSpecificConfig` `asc`.
fn aac_seq_header_data(asc: &Bytes) -> Bytes {
    let mut data = Vec::with_capacity(2 + asc.len());
    data.push(0xAF); // SoundFormat=10(AAC); rate/size/type bits ignored per spec
    data.push(0); // AACPacketType = 0 (sequence header)
    data.extend_from_slice(asc);
    Bytes::from(data)
}

/// `AudioTagHeader` + payload for one AAC raw-frame data tag (`AACPacketType=1`).
fn aac_raw_data(raw: &Bytes) -> Bytes {
    let mut data = Vec::with_capacity(2 + raw.len());
    data.push(0xAF);
    data.push(1); // AACPacketType = 1 (raw AAC frame)
    data.extend_from_slice(raw);
    Bytes::from(data)
}

/// `AudioTagHeader` + payload for one MP3 frame (`SoundFormat=2`) — MP3 has
/// no separate sequence-header sub-framing, matching `Demuxer::handle_audio`'s MP3 branch.
fn mp3_data(frame: &Bytes) -> Bytes {
    let mut data = Vec::with_capacity(1 + frame.len());
    data.push(0x2F); // SoundFormat=2(MP3); rate/size/type bits ignored per spec
    data.extend_from_slice(frame);
    Bytes::from(data)
}

/// Demuxer wrapping [`flv_core::Demuxer`] with a Mediaway stream cache.
#[derive(Debug, Default)]
pub struct Demuxer {
    inner: CoreDemuxer,
    streams: Vec<StreamInfo>,
    video_extra: Option<Bytes>,
    audio_extra: Option<Bytes>,
}

impl Demuxer {
    /// Empty demuxer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed bytes.
    pub fn push_bytes(&mut self, chunk: &[u8]) {
        self.inner.push_bytes(chunk);
    }

    /// Streams recognized so far — see module docs for codec coverage.
    #[must_use]
    pub fn streams(&self) -> &[StreamInfo] {
        &self.streams
    }

    /// Next demuxed packet. Sequence-header tags (AVC/AAC config) update the
    /// matching stream's `extra_data` and are not themselves returned as
    /// packets; tags with an unrecognized codec are skipped.
    pub fn poll_packet(&mut self) -> Option<Packet> {
        loop {
            let tag = self.inner.poll_tag().ok().flatten()?;
            match tag.tag_type {
                TagType::Video => {
                    if let Some(p) = self.handle_video(&tag) {
                        return Some(p);
                    }
                }
                TagType::Audio => {
                    if let Some(p) = self.handle_audio(&tag) {
                        return Some(p);
                    }
                }
                // Covers `ScriptData` (AMF metadata) and any future `TagType` variant.
                _ => {}
            }
        }
    }

    fn handle_video(&mut self, tag: &Tag) -> Option<Packet> {
        let (&first, rest) = tag.data.split_first()?;
        let frame_type = first >> 4;
        let codec_id = first & 0x0F;
        if codec_id != 7 {
            return None; // not AVC — no CodecKind mapping (see module docs)
        }
        if rest.len() < 4 {
            return None;
        }
        let avc_packet_type = rest[0];
        // Sign-extend the 24-bit big-endian CompositionTime field.
        let composition_time_ms = i32::from_be_bytes([0, rest[1], rest[2], rest[3]]) << 8 >> 8;
        let nalus = &rest[4..];

        match avc_packet_type {
            0 => {
                self.video_extra = Some(Bytes::copy_from_slice(nalus));
                self.sync_video_stream();
                None
            }
            1 => {
                self.sync_video_stream();
                let dts = i64::from(tag.timestamp_ms);
                Some(Packet {
                    stream_id: VIDEO_STREAM_ID,
                    pts: dts + i64::from(composition_time_ms),
                    dts,
                    duration: 0,
                    is_keyframe: frame_type == 1,
                    is_discard: false,
                    payload: Bytes::copy_from_slice(nalus),
                })
            }
            _ => None, // AVC end-of-sequence marker — no payload
        }
    }

    fn handle_audio(&mut self, tag: &Tag) -> Option<Packet> {
        let (&first, rest) = tag.data.split_first()?;
        let sound_format = first >> 4;
        match sound_format {
            2 => {
                // MP3 — no extra sequence-header sub-framing; the whole
                // remainder is one already-encoded Layer III frame.
                self.audio_extra.get_or_insert_with(Bytes::new);
                self.sync_audio_stream(CodecKind::Mp3);
                Some(Packet {
                    stream_id: AUDIO_STREAM_ID,
                    pts: i64::from(tag.timestamp_ms),
                    dts: i64::from(tag.timestamp_ms),
                    duration: 0,
                    is_keyframe: true,
                    is_discard: false,
                    payload: Bytes::copy_from_slice(rest),
                })
            }
            10 => {
                let (&aac_packet_type, aac) = rest.split_first()?;
                match aac_packet_type {
                    0 => {
                        self.audio_extra = Some(Bytes::copy_from_slice(aac));
                        self.sync_audio_stream(CodecKind::Aac);
                        None
                    }
                    1 => {
                        self.sync_audio_stream(CodecKind::Aac);
                        Some(Packet {
                            stream_id: AUDIO_STREAM_ID,
                            pts: i64::from(tag.timestamp_ms),
                            dts: i64::from(tag.timestamp_ms),
                            duration: 0,
                            is_keyframe: true,
                            is_discard: false,
                            payload: Bytes::copy_from_slice(aac),
                        })
                    }
                    _ => None,
                }
            }
            _ => None, // no CodecKind mapping (see module docs)
        }
    }

    fn sync_video_stream(&mut self) {
        if self.streams.iter().any(|s| s.id() == VIDEO_STREAM_ID) {
            return;
        }
        self.streams.push(StreamInfo::Video {
            id: VIDEO_STREAM_ID,
            codec: CodecKind::H264,
            time_base: MS_TIME_BASE,
            // FLV's AVCDecoderConfigurationRecord carries no explicit
            // width/height — real dimensions come from the SPS inside
            // `extra_data`, which this module does not parse (out of scope:
            // that is bitstream inspection, not tag/header framing).
            geometry: mediaway_common::VideoGeometry {
                width: 0,
                height: 0,
            },
            extra_data: self.video_extra.clone().unwrap_or_default(),
        });
    }

    fn sync_audio_stream(&mut self, codec: CodecKind) {
        if self.streams.iter().any(|s| s.id() == AUDIO_STREAM_ID) {
            return;
        }
        self.streams.push(StreamInfo::Audio {
            id: AUDIO_STREAM_ID,
            codec,
            time_base: MS_TIME_BASE,
            // FLV's AudioTagHeader SoundRate/SoundType are meaningless for
            // AAC (per spec) and not parsed for MP3 either — real values live
            // in the codec's own header (ASC / MP3 frame header), out of
            // scope for this tag-framing module.
            extra_data: self.audio_extra.clone().unwrap_or_default(),
            sample_rate: 0,
            channels: 0,
        });
    }
}

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

#[cfg(test)]
#[path = "flv_tests.rs"]
mod tests;
