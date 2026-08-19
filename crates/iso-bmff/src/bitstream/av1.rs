//! AV1 OBU stream → `av1C`.

#![forbid(unsafe_code)]

use bytes::Bytes;

/// `av1C` conversion result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Av1cOut {
    /// Fresh `av1C` (`AV1CodecConfigurationRecord`) payload when a Sequence Header OBU was
    /// found in the input.
    pub av1c: Option<Bytes>,
}

/// Build an `AV1CodecConfigurationRecord` (`av1C`) from a raw OBU byte stream.
///
/// Source: `av1C`, `AOMedia` AV1 Codec ISO Media File Format Binding § 2.3.3, given e.g.
/// WMF's `MF_MT_MPEG_SEQUENCE_HEADER` attribute. `marker = 1`, `version = 1`; `seq_profile` /
/// `seq_level_idx_0` / `seq_tier_0` / `high_bitdepth` / `twelve_bit` / `monochrome` /
/// `chroma_subsampling_{x,y}` / `chroma_sample_position` / `initial_presentation_delay` are
/// left zero (deferred until a real AV1 encoder MFT exists to verify field population
/// against — see `mediaway-encoder-windows` ADR-0010); `configOBUs` is the Sequence Header
/// OBU's bytes verbatim (mirroring [`super::avc::to_avcc`]'s "concatenate the raw
/// parameter-set NALs, don't re-encode them" approach). Falls back to `Av1cOut { av1c: None
/// }` — the same shape `AvccOut` uses for "not recognized" — when no Sequence Header OBU
/// (`obu_type == 1`) is found, so callers keep their existing not-a-real-config fallback
/// behavior.
#[must_use]
pub fn to_av1c(data: &[u8]) -> Av1cOut {
    let Some(seq_header) = find_sequence_header_obu(data) else {
        return Av1cOut { av1c: None };
    };
    let mut out = Vec::with_capacity(4 + seq_header.len());
    out.push(0x81); // marker = 1, version = 1
    out.extend_from_slice(&[0, 0, 0]); // profile/level/tier + remaining bitfields: zero, see doc comment above
    out.extend_from_slice(seq_header);
    Av1cOut {
        av1c: Some(Bytes::from(out)),
    }
}

/// Locate the first Sequence Header OBU (`obu_type == 1`, AV1 spec § 6.2.1) in a raw OBU
/// stream and return its bytes verbatim (`obu_header` through the end of its payload). Only
/// OBUs with `obu_has_size_field` set are walked — without an explicit size field this helper
/// cannot safely bound an OBU without parsing its payload, so it stops (returns `None`) rather
/// than guessing. Returns `None` on a malformed stream or when no Sequence Header OBU is
/// found — this reads encoder-sourced, otherwise-untrusted bytes.
fn find_sequence_header_obu(data: &[u8]) -> Option<&[u8]> {
    let mut pos = 0usize;
    while pos < data.len() {
        let header_byte = data[pos];
        if header_byte & 0x80 != 0 {
            // forbidden_bit set — not a valid OBU stream from here.
            return None;
        }
        let obu_type = (header_byte >> 3) & 0x0f;
        let extension_flag = header_byte & 0x04 != 0;
        let has_size_field = header_byte & 0x02 != 0;
        if !has_size_field {
            return None;
        }
        let header_len = if extension_flag { 2 } else { 1 };
        let (obu_size, leb_len) = read_leb128(data.get(pos + header_len..)?)?;
        let total_len = header_len.checked_add(leb_len)?.checked_add(obu_size)?;
        let obu_end = pos.checked_add(total_len)?;
        let obu = data.get(pos..obu_end)?;
        if obu_type == 1 {
            return Some(obu);
        }
        pos = obu_end;
    }
    None
}

/// Parse a `leb128`-encoded unsigned integer (AV1 spec § 4.10.5). Returns
/// `(value, bytes_consumed)`, or `None` on a truncated/overlong (>8 byte) encoding.
fn read_leb128(data: &[u8]) -> Option<(usize, usize)> {
    let mut value: u64 = 0;
    for (i, &byte) in data.iter().enumerate().take(8) {
        value |= u64::from(byte & 0x7f) << (i * 7);
        if byte & 0x80 == 0 {
            return usize::try_from(value).ok().map(|v| (v, i + 1));
        }
    }
    None
}

#[cfg(test)]
#[path = "av1_tests.rs"]
mod tests;
