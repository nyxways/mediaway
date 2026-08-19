//! HEVC (H.265) Annex-B ↔ `hvcC`.
//!
//! Mirrors [`super::avc`]'s shape (`to_avcc`/`parse_avc_decoder_config`/
//! `annex_b_sequence_header`/`avcc_payload_to_annex_b`), generalized from one NAL header byte to
//! HEVC's two and from 2 parameter-set types (SPS/PPS) to 3 (VPS/SPS/PPS).

#![forbid(unsafe_code)]

use bytes::Bytes;
use memchr::memmem;

/// `hvcC` conversion result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HvccOut {
    /// Length-prefixed access unit.
    pub payload: Bytes,
    /// Fresh `hvcC` when VPS+SPS+PPS were present.
    pub hvcc: Option<Bytes>,
}

/// Convert Annex-B to 4-byte length-prefixed `hvcC` framing, or pass through.
#[must_use]
pub fn to_hvcc(data: &[u8]) -> HvccOut {
    if !is_annex_b(data) {
        return HvccOut {
            payload: Bytes::copy_from_slice(data),
            hvcc: None,
        };
    }

    let mut out = Vec::with_capacity(data.len());
    let mut vps: Option<&[u8]> = None;
    let mut sps: Option<&[u8]> = None;
    let mut pps: Option<&[u8]> = None;

    for nal in NalIter::new(data) {
        if nal.len() < 2 {
            continue;
        }
        match nal_unit_type(nal) {
            VPS_NUT => vps = Some(nal),
            SPS_NUT => sps = Some(nal),
            PPS_NUT => pps = Some(nal),
            _ => {}
        }
        let len = u32::try_from(nal.len()).unwrap_or(u32::MAX);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(nal);
    }

    let hvcc = match (vps, sps, pps) {
        (Some(v), Some(s), Some(p)) => Some(build_hvcc(v, s, p)),
        _ => None,
    };

    HvccOut {
        payload: Bytes::from(out),
        hvcc,
    }
}

const VPS_NUT: u8 = 32;
const SPS_NUT: u8 = 33;
const PPS_NUT: u8 = 34;

/// `nal_unit_type` from a 2-byte HEVC NAL header: bits 1..6 of the first byte
/// (`forbidden_zero_bit`(1) + `nal_unit_type`(6) + `nuh_layer_id` high bit(1)).
fn nal_unit_type(nal: &[u8]) -> u8 {
    (nal[0] >> 1) & 0x3f
}

/// True when `data` starts with an Annex-B start code (`00 00 01` or `00 00 00 01`).
#[must_use]
pub fn is_annex_b(data: &[u8]) -> bool {
    matches!(find_start_code(data), Some((0, _)))
}

/// Parsed `lengthSizeMinusOne` + VPS/SPS/PPS from an `HEVCDecoderConfigurationRecord`
/// (the raw `hvcC` box payload, ISO/IEC 14496-15 § 8.3.3.1.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HevcDecoderConfig {
    /// NAL length prefix size in bytes (1, 2, or 4) used by `hvcC`-framed samples.
    pub nal_length_size: u8,
    /// One or more VPS NAL units (without start code or length prefix).
    pub vps: Vec<Bytes>,
    /// One or more SPS NAL units (without start code or length prefix).
    pub sps: Vec<Bytes>,
    /// One or more PPS NAL units (without start code or length prefix).
    pub pps: Vec<Bytes>,
}

/// Parse an `HEVCDecoderConfigurationRecord` (`hvcC` box payload). Returns `None` on
/// malformed/truncated input rather than panicking — this reads demuxer-sourced,
/// otherwise-untrusted bytes.
#[must_use]
pub fn parse_hevc_decoder_config(record: &[u8]) -> Option<HevcDecoderConfig> {
    if record.len() < 23 || record[0] != 1 {
        return None;
    }
    let nal_length_size = (record[21] & 0x03) + 1;
    let num_arrays = record[22];
    let mut pos = 23usize;

    let mut vps = Vec::new();
    let mut sps = Vec::new();
    let mut pps = Vec::new();

    for _ in 0..num_arrays {
        let array_header = *record.get(pos)?;
        let array_nal_type = array_header & 0x3f;
        pos += 1;
        let num_nalus_bytes = record.get(pos..pos + 2)?;
        let num_nalus = u16::from_be_bytes([num_nalus_bytes[0], num_nalus_bytes[1]]);
        pos += 2;
        for _ in 0..num_nalus {
            let (nal, next) = read_length_prefixed(record, pos)?;
            pos = next;
            match array_nal_type {
                VPS_NUT => vps.push(nal),
                SPS_NUT => sps.push(nal),
                PPS_NUT => pps.push(nal),
                _ => {}
            }
        }
    }

    Some(HevcDecoderConfig {
        nal_length_size,
        vps,
        sps,
        pps,
    })
}

fn read_length_prefixed(data: &[u8], pos: usize) -> Option<(Bytes, usize)> {
    let len_bytes = data.get(pos..pos + 2)?;
    let len = usize::from(u16::from_be_bytes([len_bytes[0], len_bytes[1]]));
    let start = pos + 2;
    let nal = data.get(start..start + len)?;
    Some((Bytes::copy_from_slice(nal), start + len))
}

/// Concatenated Annex-B (4-byte start code) VPS + SPS + PPS from a parsed decoder config.
#[must_use]
pub fn annex_b_sequence_header(config: &HevcDecoderConfig) -> Bytes {
    let mut out = Vec::new();
    for nal in config
        .vps
        .iter()
        .chain(config.sps.iter())
        .chain(config.pps.iter())
    {
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(nal);
    }
    Bytes::from(out)
}

/// Convert one length-prefixed access unit to Annex-B (4-byte start codes).
///
/// Stops at the first malformed/truncated NAL length rather than panicking; whatever
/// converted so far is returned (matches `to_hvcc`'s best-effort, non-`Result` style).
#[must_use]
pub fn hvcc_payload_to_annex_b(data: &[u8], nal_length_size: u8) -> Bytes {
    let nls = usize::from(nal_length_size).clamp(1, 4);
    let mut out = Vec::with_capacity(data.len() + 16);
    let mut pos = 0usize;
    while pos + nls <= data.len() {
        let Some(len) = read_nal_length(&data[pos..pos + nls]) else {
            break;
        };
        pos += nls;
        let Some(nal) = data.get(pos..pos + len) else {
            break;
        };
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(nal);
        pos += len;
    }
    Bytes::from(out)
}

fn read_nal_length(len_bytes: &[u8]) -> Option<usize> {
    match len_bytes.len() {
        1 => Some(usize::from(len_bytes[0])),
        2 => Some(usize::from(u16::from_be_bytes([
            len_bytes[0],
            len_bytes[1],
        ]))),
        4 => Some(
            usize::try_from(u32::from_be_bytes([
                len_bytes[0],
                len_bytes[1],
                len_bytes[2],
                len_bytes[3],
            ]))
            .unwrap_or(usize::MAX),
        ),
        _ => None,
    }
}

/// Build an `HEVCDecoderConfigurationRecord` (`hvcC`) from one VPS/SPS/PPS NAL each.
///
/// `general_profile_space`/`tier`/`profile_idc`/`profile_compatibility_flags`/
/// `constraint_indicator_flags`/`level_idc` are copied verbatim from the SPS's
/// `profile_tier_level()` general fields — byte-aligned at a fixed offset right after the SPS's
/// 2-byte NAL header + 1-byte `sps_video_parameter_set_id`/`sps_max_sub_layers_minus1`/
/// `sps_temporal_id_nesting_flag` byte (ITU-T H.265 § 7.3.3), so no exp-golomb parsing is needed
/// for these fields — the same "copy the known fixed-position bytes" approach [`super::avc::
/// build_avcc`] already uses for H.264's `profile_idc`/`constraint_flags`/`level_idc`.
/// `min_spatial_segmentation_idc`/`parallelismType`/`chroma_format_idc`/`bit_depth_*`/
/// `avgFrameRate`/`numTemporalLayers` sit past exp-golomb-coded fields in the SPS RBSP and are
/// left at safe defaults (4:2:0, 8-bit, one temporal layer) rather than parsed — mirrors
/// [`super::av1::to_av1c`]'s identical "informational fields default until verified against a
/// real encoder" precedent. `lengthSizeMinusOne` is always 3 (4-byte), matching this crate's
/// `to_hvcc` output framing.
fn build_hvcc(vps: &[u8], sps: &[u8], pps: &[u8]) -> Bytes {
    let mut v = Vec::with_capacity(23 + 3 * 5 + vps.len() + sps.len() + pps.len());
    v.push(1); // configurationVersion

    if sps.len() >= 15 {
        v.push(sps[3]); // general_profile_space(2) + general_tier_flag(1) + general_profile_idc(5)
        v.extend_from_slice(&sps[4..8]); // general_profile_compatibility_flags(32)
        v.extend_from_slice(&sps[8..14]); // general_constraint_indicator_flags(48)
        v.push(sps[14]); // general_level_idc
    } else {
        v.extend_from_slice(&[0u8; 12]);
    }

    v.extend_from_slice(&[0xf0, 0x00]); // reserved(4)='1111' + min_spatial_segmentation_idc(12)=0
    v.push(0xfc); // reserved(6)='111111' + parallelismType(2)=0
    v.push(0xfd); // reserved(6)='111111' + chroma_format_idc(2)=1 (4:2:0)
    v.push(0xf8); // reserved(5)='11111' + bit_depth_luma_minus8(3)=0 (8-bit)
    v.push(0xf8); // reserved(5)='11111' + bit_depth_chroma_minus8(3)=0 (8-bit)
    v.extend_from_slice(&[0, 0]); // avgFrameRate(16)=0 (unknown)
    // constantFrameRate(2)=0 | numTemporalLayers(3)=1 | temporalIdNested(1)=0 | lengthSizeMinusOne(2)=3
    v.push(0x0b);

    v.push(3); // numOfArrays: VPS, SPS, PPS

    for (nal_type, nal) in [(VPS_NUT, vps), (SPS_NUT, sps), (PPS_NUT, pps)] {
        v.push(0x80 | nal_type); // array_completeness=1, reserved=0, NAL_unit_type
        v.extend_from_slice(&1u16.to_be_bytes()); // numNalus
        v.extend_from_slice(&(u16::try_from(nal.len()).unwrap_or(u16::MAX)).to_be_bytes());
        v.extend_from_slice(nal);
    }

    Bytes::from(v)
}

/// Next Annex-B start code: `(offset, code_len)` with `code_len` in `{3, 4}`.
/// Prefers the 4-byte code when both would match at the same NAL boundary.
fn find_start_code(hay: &[u8]) -> Option<(usize, usize)> {
    let i4 = memmem::find(hay, &[0, 0, 0, 1]);
    let i3 = memmem::find(hay, &[0, 0, 1]);
    match (i4, i3) {
        (Some(a), Some(b)) if a <= b => Some((a, 4)),
        (Some(a), None) => Some((a, 4)),
        (_, Some(b)) => Some((b, 3)),
        (None, None) => None,
    }
}

/// Zero-alloc Annex-B NAL iterator (yields slices into the input).
struct NalIter<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> NalIter<'a> {
    const fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
}

impl<'a> Iterator for NalIter<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        let data = self.data;
        loop {
            if self.pos >= data.len() {
                return None;
            }
            let (sc_at, sc_len) = find_start_code(&data[self.pos..])?;
            let start = self.pos + sc_at + sc_len;
            let end = match find_start_code(&data[start..]) {
                Some((rel, _)) => start + rel,
                None => data.len(),
            };
            self.pos = end;
            if start < end {
                return Some(&data[start..end]);
            }
        }
    }
}

#[cfg(test)]
#[path = "hevc_tests.rs"]
mod tests;
