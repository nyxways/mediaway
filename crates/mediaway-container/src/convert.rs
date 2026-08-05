//! Map [`iso_bmff`] types ↔ [`mediaway_common`].

#![forbid(unsafe_code)]

use iso_bmff::{Codec, Rational, Sample, Track};
use mediaway_common::{CodecKind, Packet, Rational as MwRational, StreamInfo, VideoGeometry};

/// Convert an ISOBMFF track to Mediaway [`StreamInfo`].
#[must_use]
pub fn to_stream_info(t: &Track) -> StreamInfo {
    let codec = to_codec_kind(t.codec);
    let time_base = MwRational::new(t.time_base.num, t.time_base.den);
    // clone: facade returns owned StreamInfo; Track keeps its Bytes
    let extra_data = t.extra_data.clone();
    if codec.is_video() {
        StreamInfo::Video {
            id: t.id,
            codec,
            time_base,
            geometry: VideoGeometry {
                width: t.width,
                height: t.height,
            },
            extra_data,
        }
    } else {
        StreamInfo::Audio {
            id: t.id,
            codec,
            time_base,
            extra_data,
            // `iso_bmff::Track` doesn't carry sample rate / channel count yet
            // (MP4 sample-entry writers infer these from `time_base`/hardcode
            // them — see `sample_entry.rs::write_mp4a`); not fabricated here.
            sample_rate: 0,
            channels: 0,
        }
    }
}

/// Convert Mediaway [`StreamInfo`] to an ISOBMFF [`Track`].
#[must_use]
pub fn from_stream_info(s: &StreamInfo) -> Track {
    let (width, height) = s.geometry().map_or((0, 0), |g| (g.width, g.height));
    let time_base = s.time_base();
    Track {
        id: s.id(),
        codec: from_codec_kind(s.codec()),
        time_base: Rational::new(time_base.num, time_base.den),
        width,
        height,
        // clone: Track owns codec config; StreamInfo is independent after convert
        extra_data: s.extra_data().clone(),
    }
}

/// Convert an ISOBMFF sample to Mediaway [`Packet`].
#[must_use]
pub fn to_packet(s: Sample) -> Packet {
    Packet {
        stream_id: s.stream_id,
        pts: s.pts,
        dts: s.dts,
        duration: s.duration,
        is_keyframe: s.is_keyframe,
        is_discard: s.is_discard,
        payload: s.payload,
    }
}

/// Convert Mediaway [`Packet`] to an ISOBMFF [`Sample`].
#[must_use]
pub fn from_packet(p: &Packet) -> Sample {
    Sample {
        stream_id: p.stream_id,
        pts: p.pts,
        dts: p.dts,
        duration: p.duration,
        is_keyframe: p.is_keyframe,
        is_discard: p.is_discard,
        // clone: Sample needs owned payload; Packet may be reused by caller
        payload: p.payload.clone(),
    }
}

const fn to_codec_kind(c: Codec) -> CodecKind {
    match c {
        Codec::Hevc => CodecKind::Hevc,
        Codec::Av1 => CodecKind::Av1,
        Codec::Vp9 => CodecKind::Vp9,
        Codec::Aac => CodecKind::Aac,
        Codec::Opus => CodecKind::Opus,
        Codec::WebVtt => CodecKind::WebVtt,
        Codec::Tx3g => CodecKind::Tx3g,
        // `H264` and any future `Codec` variant until explicitly mapped.
        _ => CodecKind::H264,
    }
}

const fn from_codec_kind(c: CodecKind) -> Codec {
    match c {
        CodecKind::Hevc => Codec::Hevc,
        CodecKind::Av1 => Codec::Av1,
        CodecKind::Vp9 => Codec::Vp9,
        CodecKind::Aac => Codec::Aac,
        CodecKind::Opus => Codec::Opus,
        CodecKind::WebVtt => Codec::WebVtt,
        CodecKind::Tx3g => Codec::Tx3g,
        // Raw capture, MP3, Vorbis, and VP8 are not ISOBMFF sample codecs this
        // crate writes (VP8 is WebM/Matroska's domain, not MP4's); do not mux
        // them via this helper.
        CodecKind::H264
        | CodecKind::RawVideo
        | CodecKind::RawAudio
        | CodecKind::Mp3
        | CodecKind::Vorbis
        | CodecKind::Vp8 => Codec::H264,
    }
}
