//! Convenience ISOBMFF box tree walk (probe / debug).

#![forbid(unsafe_code)]

use crate::mp4::isobmff::header::{Header, parse_header};

/// Parsed box node for tree printing.
#[derive(Debug, Clone)]
pub struct Mp4BoxNode {
    /// Header.
    pub header: ParsedBoxHeader,
    /// Absolute payload offset in the original buffer.
    pub payload_offset: usize,
    /// Children.
    pub children: Vec<Self>,
}

/// Header shape for tree nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedBoxHeader {
    /// Tag bytes.
    pub tag: [u8; 4],
    /// Total size.
    pub size: usize,
    /// Header size.
    pub header_size: usize,
}

impl From<Header> for ParsedBoxHeader {
    fn from(h: Header) -> Self {
        Self {
            tag: h.typ.0,
            size: h.size,
            header_size: h.header_len,
        }
    }
}

/// Parse nested boxes (container tags only).
#[must_use]
pub fn parse_box_tree(buf: &[u8], base_offset: usize) -> Vec<Mp4BoxNode> {
    let mut nodes = Vec::new();
    let mut offset = 0;
    while offset + 8 <= buf.len() {
        let Some(h) = parse_header(&buf[offset..]) else {
            break;
        };
        if offset + h.size > buf.len() || h.size == 0 {
            break;
        }
        let payload_start = offset + h.header_len;
        let box_end = offset + h.size;
        let is_container = matches!(
            &h.typ.0,
            b"moov" | b"trak" | b"mdia" | b"minf" | b"dinf" | b"stbl" | b"mvex" | b"moof" | b"traf"
        );
        let children = if is_container && payload_start < box_end {
            parse_box_tree(&buf[payload_start..box_end], base_offset + payload_start)
        } else {
            Vec::new()
        };
        nodes.push(Mp4BoxNode {
            header: h.into(),
            payload_offset: base_offset + payload_start,
            children,
        });
        offset = box_end;
    }
    nodes
}
