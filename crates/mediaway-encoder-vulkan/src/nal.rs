//! Minimal Annex-B NAL-unit scanner used only to *verify* Stage 1's encode
//! output looks like real H.264 (start code + NAL type), not to decode it.
//!
//! Deliberately not a general Annex-B parser: no emulation-prevention-byte
//! stripping, no RBSP extraction — this crate only needs to confirm the
//! sequence of NAL types present in a byte buffer.

#![allow(
    clippy::redundant_pub_crate,
    reason = "workspace `unreachable_pub` policy (Cargo.toml) wants `pub(crate)` here; \
              clippy::pedantic's redundant_pub_crate disagrees for private modules — the \
              two lints are mutually exclusive for this shape, workspace policy wins"
)]

/// One Annex-B NAL unit's `nal_unit_type` (low 5 bits of the byte right after
/// the start code) and the byte offset that type byte was found at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NalHeader {
    pub(crate) nal_unit_type: u8,
    pub(crate) offset: usize,
}

/// Scans `data` for `00 00 01` / `00 00 00 01` Annex-B start codes and
/// returns the H.264 `nal_unit_type` (low 5 bits of the 1-byte NAL header)
/// following each one, in order.
///
/// Stops at the first run of the buffer that contains no further start code
/// (e.g. the zero-filled tail Stage 1 leaves past the driver's actual
/// bitstream output — see `session.rs`'s doc comment on why the destination
/// buffer isn't byte-exact-trimmed).
pub(crate) fn scan_nal_headers(data: &[u8]) -> Vec<NalHeader> {
    scan_nal_headers_generic(data, 1, |type_byte| type_byte & 0x1F)
}

/// HEVC sibling of [`scan_nal_headers`] — the NAL header is 2 bytes
/// (`forbidden_zero_bit`(1) + `nal_unit_type`(6) + `nuh_layer_id`(6) +
/// `nuh_temporal_id_plus1`(3), Rec. ITU-T H.265 §7.3.1.2), so
/// `nal_unit_type` is the top 6 bits of the first header byte.
pub(crate) fn scan_nal_headers_hevc(data: &[u8]) -> Vec<NalHeader> {
    scan_nal_headers_generic(data, 2, |first_byte| (first_byte >> 1) & 0x3F)
}

/// One AV1 OBU's `obu_type` (bits 6..3 of the header byte, AV1 spec §5.3.2)
/// and the byte offset that header byte was found at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ObuHeader {
    pub(crate) obu_type: u8,
    pub(crate) offset: usize,
}

/// Scans one low-overhead-format AV1 OBU stream — every OBU sets
/// `obu_has_size_field` (the conventional packing for raw/elementary AV1
/// streams, and what this crate's own encode output produces) — and returns
/// each OBU's `obu_type`, in order.
///
/// Stops (without erroring) at the first OBU that does not set
/// `obu_has_size_field`, or whose declared size runs past the end of `data`
/// — this is a verification scanner, not a decoder, mirroring
/// [`scan_nal_headers_generic`]'s "stop at the first unparseable run" shape
/// (this crate's destination buffer is query-pool-trimmed to the driver's
/// real byte count, so no zero-padded tail is expected here, unlike the
/// Annex-B scanners' historical reason for that shape).
pub(crate) fn scan_obu_headers(data: &[u8]) -> Vec<ObuHeader> {
    let mut headers = Vec::new();
    let mut i = 0usize;
    while i < data.len() {
        let header_byte = data[i];
        let obu_type = (header_byte >> 3) & 0x0F;
        let has_extension = (header_byte >> 2) & 1 == 1;
        let has_size_field = (header_byte >> 1) & 1 == 1;
        headers.push(ObuHeader {
            obu_type,
            offset: i,
        });

        let mut cursor = i + 1;
        if has_extension {
            cursor += 1;
        }
        if !has_size_field {
            break;
        }
        let Some((obu_size, leb_len)) = read_leb128(data.get(cursor..).unwrap_or(&[])) else {
            break;
        };
        cursor += leb_len;
        let Some(obu_size) = usize::try_from(obu_size).ok() else {
            break;
        };
        let Some(next) = cursor.checked_add(obu_size) else {
            break;
        };
        if next > data.len() {
            break;
        }
        i = next;
    }
    headers
}

/// Minimal AV1 LEB128 decoder (AV1 spec §4.10.5) — returns
/// `(value, bytes_consumed)`, `None` if `data` runs out before a terminating
/// (high-bit-clear) byte within the spec's 8-byte maximum.
fn read_leb128(data: &[u8]) -> Option<(u64, usize)> {
    let mut value: u64 = 0;
    for (i, &byte) in data.iter().enumerate().take(8) {
        value |= u64::from(byte & 0x7F) << (i * 7);
        if byte & 0x80 == 0 {
            return Some((value, i + 1));
        }
    }
    None
}

/// Shared Annex-B start-code scanner — `header_len` is the NAL header's
/// byte width (1 for H.264, 2 for HEVC) and `extract_type` maps the header's
/// first byte to `nal_unit_type`.
fn scan_nal_headers_generic(
    data: &[u8],
    header_len: usize,
    extract_type: impl Fn(u8) -> u8,
) -> Vec<NalHeader> {
    let mut headers = Vec::new();
    let mut i = 0usize;
    while i + 3 <= data.len() {
        let is_start_code_3 = data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1;
        let is_start_code_4 = i + 4 <= data.len()
            && data[i] == 0
            && data[i + 1] == 0
            && data[i + 2] == 0
            && data[i + 3] == 1;
        let start_code_len = if is_start_code_4 {
            4
        } else if is_start_code_3 {
            3
        } else {
            0
        };
        if start_code_len == 0 {
            i += 1;
            continue;
        }
        let type_offset = i + start_code_len;
        if type_offset + header_len > data.len() {
            break;
        }
        if let Some(&first_byte) = data.get(type_offset) {
            headers.push(NalHeader {
                nal_unit_type: extract_type(first_byte),
                offset: type_offset,
            });
        }
        i = type_offset + 1;
    }
    headers
}
