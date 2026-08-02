//! AAC ADTS framing.

#![forbid(unsafe_code)]

use bytes::Bytes;

/// Strip ADTS if present; return raw AAC and optional `AudioSpecificConfig`.
#[must_use]
pub fn strip_adts(data: &[u8]) -> (Bytes, Option<Bytes>) {
    if data.len() < 7 || data[0] != 0xff || (data[1] & 0xf0) != 0xf0 {
        return (Bytes::copy_from_slice(data), None);
    }
    let protection_absent = (data[1] & 0x01) != 0;
    let header_len = if protection_absent { 7 } else { 9 };
    if data.len() < header_len {
        return (Bytes::copy_from_slice(data), None);
    }
    let object_type = ((data[2] >> 6) & 0x03) + 1;
    let sample_rate_index = (data[2] >> 2) & 0x0f;
    let channels = ((data[2] & 0x01) << 2) | ((data[3] >> 6) & 0x03);
    let a = (object_type << 3) | (sample_rate_index >> 1);
    let b = (sample_rate_index << 7) | (channels << 3);
    (
        Bytes::copy_from_slice(&data[header_len..]),
        Some(Bytes::from(vec![a, b])),
    )
}
