//! Box header parse / write.

#![forbid(unsafe_code)]

use super::buf::ByteSource;
use super::fourcc::FourCc;

/// Parsed ISOBMFF box header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// Box type.
    pub typ: FourCc,
    /// Total size including header.
    pub size: usize,
    /// Header length (8 or 16).
    pub header_len: usize,
}

/// Parse a box header at the start of `data`.
#[must_use]
pub fn parse_header(data: &[u8]) -> Option<Header> {
    let mut src = ByteSource::new(data);
    let size32 = src.u32()? as usize;
    let typ = FourCc([src.u8()?, src.u8()?, src.u8()?, src.u8()?]);
    if size32 == 1 {
        let size64 = src.u64()? as usize;
        Some(Header {
            typ,
            size: size64,
            header_len: 16,
        })
    } else if size32 == 0 {
        None
    } else {
        Some(Header {
            typ,
            size: size32,
            header_len: 8,
        })
    }
}

/// Write a box: placeholder size, `body`, then patch size.
pub fn write_box(buf: &mut Vec<u8>, typ: FourCc, body: impl FnOnce(&mut Vec<u8>)) {
    let start = buf.len();
    buf.extend_from_slice(&0u32.to_be_bytes());
    buf.extend_from_slice(&typ.0);
    body(buf);
    let size = u32::try_from(buf.len().saturating_sub(start)).unwrap_or(u32::MAX);
    if let Some(slot) = buf.get_mut(start..start + 4) {
        slot.copy_from_slice(&size.to_be_bytes());
    }
}
