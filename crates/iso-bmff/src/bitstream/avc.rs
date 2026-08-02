//! H.264 Annex-B ↔ AVCC.

#![forbid(unsafe_code)]

use bytes::Bytes;
use memchr::memmem;

/// AVCC conversion result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvccOut {
    /// Length-prefixed access unit.
    pub payload: Bytes,
    /// Fresh `avcC` when SPS+PPS were present.
    pub avcc: Option<Bytes>,
}

/// Convert Annex-B to 4-byte length-prefixed AVCC, or pass through.
#[must_use]
pub fn to_avcc(data: &[u8]) -> AvccOut {
    if !is_annex_b(data) {
        return AvccOut {
            payload: Bytes::copy_from_slice(data),
            avcc: None,
        };
    }

    let mut out = Vec::with_capacity(data.len());
    let mut sps: Option<&[u8]> = None;
    let mut pps: Option<&[u8]> = None;

    for nal in NalIter::new(data) {
        if nal.is_empty() {
            continue;
        }
        match nal[0] & 0x1f {
            7 => sps = Some(nal),
            8 => pps = Some(nal),
            _ => {}
        }
        let len = u32::try_from(nal.len()).unwrap_or(u32::MAX);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(nal);
    }

    let avcc = match (sps, pps) {
        (Some(s), Some(p)) => Some(build_avcc(s, p)),
        _ => None,
    };

    AvccOut {
        payload: Bytes::from(out),
        avcc,
    }
}

fn is_annex_b(data: &[u8]) -> bool {
    matches!(find_start_code(data), Some((0, _)))
}

/// Parsed `lengthSizeMinusOne` + SPS/PPS from an `AVCDecoderConfigurationRecord`
/// (the raw `avcC` box payload, ISO/IEC 14496-15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvcDecoderConfig {
    /// NAL length prefix size in bytes (1, 2, or 4) used by AVCC-framed samples.
    pub nal_length_size: u8,
    /// One or more SPS NAL units (without start code or length prefix).
    pub sps: Vec<Bytes>,
    /// One or more PPS NAL units (without start code or length prefix).
    pub pps: Vec<Bytes>,
}

/// Parse an `AVCDecoderConfigurationRecord` (`avcC` box payload). Returns `None` on
/// malformed/truncated input rather than panicking — this reads demuxer-sourced,
/// otherwise-untrusted bytes.
#[must_use]
pub fn parse_avc_decoder_config(record: &[u8]) -> Option<AvcDecoderConfig> {
    if record.len() < 7 || record[0] != 1 {
        return None;
    }
    let nal_length_size = (record[4] & 0x03) + 1;
    let num_sps = record[5] & 0x1f;
    let mut pos = 6usize;
    let mut sps = Vec::new();
    for _ in 0..num_sps {
        let (nal, next) = read_length_prefixed(record, pos)?;
        sps.push(nal);
        pos = next;
    }
    let pps_count = *record.get(pos)?;
    pos += 1;
    let mut pps = Vec::new();
    for _ in 0..pps_count {
        let (nal, next) = read_length_prefixed(record, pos)?;
        pps.push(nal);
        pos = next;
    }
    Some(AvcDecoderConfig {
        nal_length_size,
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

/// Concatenated Annex-B (4-byte start code) SPS + PPS from a parsed decoder config —
/// the sequence-header form Windows Media Foundation's `MF_MT_MPEG_SEQUENCE_HEADER`
/// attribute expects.
#[must_use]
pub fn annex_b_sequence_header(config: &AvcDecoderConfig) -> Bytes {
    let mut out = Vec::new();
    for nal in config.sps.iter().chain(config.pps.iter()) {
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(nal);
    }
    Bytes::from(out)
}

/// Convert one AVCC length-prefixed access unit to Annex-B (4-byte start codes).
///
/// Stops at the first malformed/truncated NAL length rather than panicking; whatever
/// converted so far is returned (matches `to_avcc`'s best-effort, non-`Result` style).
#[must_use]
pub fn avcc_payload_to_annex_b(data: &[u8], nal_length_size: u8) -> Bytes {
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

fn build_avcc(sps: &[u8], pps: &[u8]) -> Bytes {
    let mut v = Vec::with_capacity(11 + sps.len() + pps.len());
    v.push(1);
    if sps.len() >= 4 {
        v.extend_from_slice(&sps[1..4]);
    } else {
        v.extend_from_slice(&[0x42, 0x00, 0x1e]);
    }
    v.push(0xff);
    v.push(0xe1);
    v.extend_from_slice(&(u16::try_from(sps.len()).unwrap_or(u16::MAX)).to_be_bytes());
    v.extend_from_slice(sps);
    v.push(1);
    v.extend_from_slice(&(u16::try_from(pps.len()).unwrap_or(u16::MAX)).to_be_bytes());
    v.extend_from_slice(pps);
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
#[path = "avc_tests.rs"]
mod tests;
