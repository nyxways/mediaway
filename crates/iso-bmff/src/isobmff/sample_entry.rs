//! Sample entries: `avc1`/`avcC`, `vp09`/`vpcC`, `mp4a`/`esds`, and encrypted `encv`/`enca`.

#![forbid(unsafe_code)]

use super::cenc_box::{TrackEncryption, parse_tenc};
use super::{parse_header, tag, write_box};
use crate::types::{Bytes, Codec, Track};

/// Write `stsd` with one sample entry for `track`.
pub(crate) fn write_stsd(buf: &mut Vec<u8>, track: &Track) {
    let audio = matches!(track.codec, Codec::Aac | Codec::Opus);
    write_box(buf, tag::STSD, |st| {
        st.extend_from_slice(&0u32.to_be_bytes());
        st.extend_from_slice(&1u32.to_be_bytes());
        if audio {
            write_mp4a(st, track);
        } else if track.codec == Codec::Vp9 {
            write_vp09(st, track);
        } else if track.codec == Codec::Hevc {
            write_hvc1(st, track);
        } else if track.codec == Codec::Av1 {
            write_av01(st, track);
        } else {
            write_avc1(st, track);
        }
    });
}

/// Parse the first sample entry after `stsd` version/count; fills codec metadata.
pub(crate) fn parse_sample_entry(
    data: &[u8],
    width: &mut u32,
    height: &mut u32,
    codec: &mut Codec,
    extra: &mut Bytes,
    encryption: &mut Option<TrackEncryption>,
) {
    let Some(hdr) = parse_header(data) else {
        return;
    };
    if hdr.size > data.len() {
        return;
    }
    let body = &data[hdr.header_len..hdr.size];
    match &hdr.typ.0 {
        b"avc1" | b"avc3" => parse_visual_avc(body, width, height, codec, extra),
        b"vp09" => parse_visual_vp9(body, width, height, codec, extra),
        b"hvc1" | b"hev1" => parse_visual_hevc(body, width, height, codec, extra),
        b"av01" => parse_visual_av1(body, width, height, codec, extra),
        b"mp4a" => parse_audio_mp4a(body, codec, extra),
        b"encv" => {
            parse_visual_avc(body, width, height, codec, extra);
            if let Some(tenc) = find_tenc(body) {
                *encryption = Some(tenc);
            }
            if let Some(frma) = find_nested_payload(body, *b"frma") {
                if frma.len() >= 4 {
                    match &frma[..4] {
                        b"avc1" | b"avc3" => *codec = Codec::H264,
                        _ => {}
                    }
                }
            }
        }
        b"enca" => {
            parse_audio_mp4a(body, codec, extra);
            if let Some(tenc) = find_tenc(body) {
                *encryption = Some(tenc);
            }
        }
        _ => {}
    }
}

fn parse_visual_avc(
    body: &[u8],
    width: &mut u32,
    height: &mut u32,
    codec: &mut Codec,
    extra: &mut Bytes,
) {
    *codec = Codec::H264;
    if body.len() >= 28 {
        *width = u32::from(u16::from_be_bytes([body[24], body[25]]));
        *height = u32::from(u16::from_be_bytes([body[26], body[27]]));
    }
    if body.len() > 78 {
        if let Some(avcc) = find_child_payload(&body[78..], *b"avcC") {
            *extra = Bytes::copy_from_slice(avcc);
        }
    }
}

fn parse_visual_vp9(
    body: &[u8],
    width: &mut u32,
    height: &mut u32,
    codec: &mut Codec,
    extra: &mut Bytes,
) {
    *codec = Codec::Vp9;
    if body.len() >= 28 {
        *width = u32::from(u16::from_be_bytes([body[24], body[25]]));
        *height = u32::from(u16::from_be_bytes([body[26], body[27]]));
    }
    if body.len() > 78 {
        if let Some(vpcc) = find_child_payload(&body[78..], *b"vpcC") {
            *extra = Bytes::copy_from_slice(vpcc);
        }
    }
}

fn parse_visual_hevc(
    body: &[u8],
    width: &mut u32,
    height: &mut u32,
    codec: &mut Codec,
    extra: &mut Bytes,
) {
    *codec = Codec::Hevc;
    if body.len() >= 28 {
        *width = u32::from(u16::from_be_bytes([body[24], body[25]]));
        *height = u32::from(u16::from_be_bytes([body[26], body[27]]));
    }
    if body.len() > 78 {
        if let Some(hvcc) = find_child_payload(&body[78..], *b"hvcC") {
            *extra = Bytes::copy_from_slice(hvcc);
        }
    }
}

fn parse_visual_av1(
    body: &[u8],
    width: &mut u32,
    height: &mut u32,
    codec: &mut Codec,
    extra: &mut Bytes,
) {
    *codec = Codec::Av1;
    if body.len() >= 28 {
        *width = u32::from(u16::from_be_bytes([body[24], body[25]]));
        *height = u32::from(u16::from_be_bytes([body[26], body[27]]));
    }
    if body.len() > 78 {
        if let Some(av1c) = find_child_payload(&body[78..], *b"av1C") {
            *extra = Bytes::copy_from_slice(av1c);
        }
    }
}

fn parse_audio_mp4a(body: &[u8], codec: &mut Codec, extra: &mut Bytes) {
    *codec = Codec::Aac;
    if body.len() > 28 {
        if let Some(esds) = find_child_payload(&body[28..], *b"esds") {
            if let Some(asc) = find_asc(esds) {
                *extra = asc;
            }
        }
    }
}

fn find_tenc(body: &[u8]) -> Option<TrackEncryption> {
    let tenc = find_nested_payload(body, *b"tenc")?;
    parse_tenc(tenc)
}

fn write_avc1(buf: &mut Vec<u8>, track: &Track) {
    write_box(buf, tag::AVC1, |a| {
        a.extend_from_slice(&[0u8; 6]);
        a.extend_from_slice(&1u16.to_be_bytes());
        a.extend_from_slice(&0u16.to_be_bytes());
        a.extend_from_slice(&0u16.to_be_bytes());
        a.extend_from_slice(&[0u8; 12]);
        a.extend_from_slice(&(track.width.min(u32::from(u16::MAX)) as u16).to_be_bytes());
        a.extend_from_slice(&(track.height.min(u32::from(u16::MAX)) as u16).to_be_bytes());
        a.extend_from_slice(&0x0048_0000u32.to_be_bytes());
        a.extend_from_slice(&0x0048_0000u32.to_be_bytes());
        a.extend_from_slice(&0u32.to_be_bytes());
        a.extend_from_slice(&1u16.to_be_bytes());
        a.extend_from_slice(&[0u8; 32]);
        a.extend_from_slice(&0x0018u16.to_be_bytes());
        a.extend_from_slice(&0xffffu16.to_be_bytes());
        write_box(a, tag::AVCC, |c| {
            if track.extra_data.is_empty() {
                c.extend_from_slice(&[1, 0x42, 0x00, 0x1e, 0xff, 0xe1, 0, 0, 1, 0, 0]);
            } else {
                c.extend_from_slice(&track.extra_data);
            }
        });
    });
}

/// Placeholder `vpcC` (VP9) config: profile 0, level 1.0, 8-bit 4:2:0, unspecified
/// colour metadata, no codec-initialization data. VP9 carries no out-of-band SPS/PPS
/// equivalent, so — unlike `avcC` — there is no real bitstream config to fall back to;
/// used whenever `track.extra_data` (a demuxed `vpcC` payload) is unavailable.
const VPCC_PLACEHOLDER: &[u8] = &[1, 0, 0, 0, 0, 10, 0x82, 2, 2, 2, 0, 0];

fn write_vp09(buf: &mut Vec<u8>, track: &Track) {
    write_box(buf, tag::VP09, |a| {
        a.extend_from_slice(&[0u8; 6]);
        a.extend_from_slice(&1u16.to_be_bytes());
        a.extend_from_slice(&0u16.to_be_bytes());
        a.extend_from_slice(&0u16.to_be_bytes());
        a.extend_from_slice(&[0u8; 12]);
        a.extend_from_slice(&(track.width.min(u32::from(u16::MAX)) as u16).to_be_bytes());
        a.extend_from_slice(&(track.height.min(u32::from(u16::MAX)) as u16).to_be_bytes());
        a.extend_from_slice(&0x0048_0000u32.to_be_bytes());
        a.extend_from_slice(&0x0048_0000u32.to_be_bytes());
        a.extend_from_slice(&0u32.to_be_bytes());
        a.extend_from_slice(&1u16.to_be_bytes());
        a.extend_from_slice(&[0u8; 32]);
        a.extend_from_slice(&0x0018u16.to_be_bytes());
        a.extend_from_slice(&0xffffu16.to_be_bytes());
        write_box(a, tag::VPCC, |c| {
            if track.extra_data.is_empty() {
                c.extend_from_slice(VPCC_PLACEHOLDER);
            } else {
                c.extend_from_slice(&track.extra_data);
            }
        });
    });
}

/// Placeholder `hvcC` (`HEVCDecoderConfigurationRecord`, ISO/IEC 14496-15):
/// version 1, profile/tier/level and all reserved fields zeroed,
/// `lengthSizeMinusOne = 3` (4-byte NAL lengths, matching this crate's AVCC-style
/// framing convention), zero parameter-set arrays. Used whenever `track.extra_data`
/// (a demuxed `hvcC` payload carrying the real VPS/SPS/PPS) is unavailable.
const HVCC_PLACEHOLDER: &[u8] = &[
    1, // configurationVersion
    0, // general_profile_space(2) + general_tier_flag(1) + general_profile_idc(5)
    0, 0, 0, 0, // general_profile_compatibility_flags
    0, 0, 0, 0, 0, 0, // general_constraint_indicator_flags
    0, // general_level_idc
    0xF0, 0x00, // reserved(4)='1111' + min_spatial_segmentation_idc(12)
    0xFC, // reserved(6)='111111' + parallelismType(2)
    0xFC, // reserved(6)='111111' + chroma_format_idc(2)
    0xF8, // reserved(5)='11111' + bit_depth_luma_minus8(3)
    0xF8, // reserved(5)='11111' + bit_depth_chroma_minus8(3)
    0, 0,    // avgFrameRate
    0x03, // constantFrameRate(2)=0 + numTemporalLayers(3)=0 + temporalIdNested(1)=0
    //      + lengthSizeMinusOne(2)=3
    0, // numOfArrays
];

fn write_hvc1(buf: &mut Vec<u8>, track: &Track) {
    write_box(buf, tag::HVC1, |a| {
        a.extend_from_slice(&[0u8; 6]);
        a.extend_from_slice(&1u16.to_be_bytes());
        a.extend_from_slice(&0u16.to_be_bytes());
        a.extend_from_slice(&0u16.to_be_bytes());
        a.extend_from_slice(&[0u8; 12]);
        a.extend_from_slice(&(track.width.min(u32::from(u16::MAX)) as u16).to_be_bytes());
        a.extend_from_slice(&(track.height.min(u32::from(u16::MAX)) as u16).to_be_bytes());
        a.extend_from_slice(&0x0048_0000u32.to_be_bytes());
        a.extend_from_slice(&0x0048_0000u32.to_be_bytes());
        a.extend_from_slice(&0u32.to_be_bytes());
        a.extend_from_slice(&1u16.to_be_bytes());
        a.extend_from_slice(&[0u8; 32]);
        a.extend_from_slice(&0x0018u16.to_be_bytes());
        a.extend_from_slice(&0xffffu16.to_be_bytes());
        write_box(a, tag::HVCC, |c| {
            if track.extra_data.is_empty() {
                c.extend_from_slice(HVCC_PLACEHOLDER);
            } else {
                c.extend_from_slice(&track.extra_data);
            }
        });
    });
}

/// Placeholder `av1C` (`AV1CodecConfigurationRecord`, `AOMedia` AV1 Codec ISO Media
/// File Format Binding § 2.3.3): `marker=1`, `version=1`, profile/level/tier and all
/// other fields zeroed, no `configOBUs`. Used whenever `track.extra_data` (a demuxed
/// `av1C` payload carrying the real Sequence Header OBU fields) is unavailable.
const AV1C_PLACEHOLDER: &[u8] = &[0x81, 0, 0, 0];

fn write_av01(buf: &mut Vec<u8>, track: &Track) {
    write_box(buf, tag::AV01, |a| {
        a.extend_from_slice(&[0u8; 6]);
        a.extend_from_slice(&1u16.to_be_bytes());
        a.extend_from_slice(&0u16.to_be_bytes());
        a.extend_from_slice(&0u16.to_be_bytes());
        a.extend_from_slice(&[0u8; 12]);
        a.extend_from_slice(&(track.width.min(u32::from(u16::MAX)) as u16).to_be_bytes());
        a.extend_from_slice(&(track.height.min(u32::from(u16::MAX)) as u16).to_be_bytes());
        a.extend_from_slice(&0x0048_0000u32.to_be_bytes());
        a.extend_from_slice(&0x0048_0000u32.to_be_bytes());
        a.extend_from_slice(&0u32.to_be_bytes());
        a.extend_from_slice(&1u16.to_be_bytes());
        a.extend_from_slice(&[0u8; 32]);
        a.extend_from_slice(&0x0018u16.to_be_bytes());
        a.extend_from_slice(&0xffffu16.to_be_bytes());
        write_box(a, tag::AV1C, |c| {
            if track.extra_data.is_empty() {
                c.extend_from_slice(AV1C_PLACEHOLDER);
            } else {
                c.extend_from_slice(&track.extra_data);
            }
        });
    });
}

fn write_mp4a(buf: &mut Vec<u8>, track: &Track) {
    write_box(buf, tag::MP4A, |a| {
        a.extend_from_slice(&[0u8; 6]);
        a.extend_from_slice(&1u16.to_be_bytes());
        a.extend_from_slice(&[0u8; 8]);
        a.extend_from_slice(&2u16.to_be_bytes());
        a.extend_from_slice(&16u16.to_be_bytes());
        a.extend_from_slice(&0u32.to_be_bytes());
        a.extend_from_slice(&(track.time_base.den << 16).to_be_bytes());
        write_box(a, tag::ESDS, |e| {
            e.extend_from_slice(&0u32.to_be_bytes());
            let asc: &[u8] = if track.extra_data.is_empty() {
                &[0x12, 0x10]
            } else {
                &track.extra_data
            };
            e.push(0x03);
            e.push(u8::try_from(asc.len() + 23).unwrap_or(u8::MAX));
            e.extend_from_slice(&(track.id as u16 + 1).to_be_bytes());
            e.push(0);
            e.push(0x04);
            e.push(u8::try_from(asc.len() + 15).unwrap_or(u8::MAX));
            e.push(0x40);
            e.push(0x15);
            e.extend_from_slice(&[0u8; 3]);
            e.extend_from_slice(&128_000u32.to_be_bytes());
            e.extend_from_slice(&128_000u32.to_be_bytes());
            e.push(0x05);
            e.push(u8::try_from(asc.len()).unwrap_or(u8::MAX));
            e.extend_from_slice(asc);
        });
    });
}

fn find_child_payload(data: &[u8], box_tag: [u8; 4]) -> Option<&[u8]> {
    let mut pos = 0;
    while pos + 8 <= data.len() {
        let hdr = parse_header(&data[pos..])?;
        if pos + hdr.size > data.len() {
            return None;
        }
        if hdr.typ.0 == box_tag {
            return Some(&data[pos + hdr.header_len..pos + hdr.size]);
        }
        pos += hdr.size;
    }
    None
}

fn find_nested_payload(data: &[u8], box_tag: [u8; 4]) -> Option<&[u8]> {
    let mut pos = 0;
    while pos + 8 <= data.len() {
        let hdr = parse_header(&data[pos..])?;
        if pos + hdr.size > data.len() {
            return None;
        }
        let payload = &data[pos + hdr.header_len..pos + hdr.size];
        if hdr.typ.0 == box_tag {
            return Some(payload);
        }
        if let Some(found) = find_nested_payload(payload, box_tag) {
            return Some(found);
        }
        pos += hdr.size;
    }
    None
}

fn find_asc(esds: &[u8]) -> Option<Bytes> {
    let start = if esds.len() >= 6 { 4 } else { 0 };
    let mut i = start;
    while i + 1 < esds.len() {
        if esds[i] == 0x05 {
            let len = usize::from(esds[i + 1]);
            let s = i + 2;
            let e = s.saturating_add(len).min(esds.len());
            if s < e {
                return Some(Bytes::copy_from_slice(&esds[s..e]));
            }
            return None;
        }
        i += 1;
    }
    None
}

#[cfg(test)]
#[path = "sample_entry_tests.rs"]
mod tests;
