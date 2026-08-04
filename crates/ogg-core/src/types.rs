//! Freestanding Ogg packet type (no Mediaway dependency).

#![forbid(unsafe_code)]

use bytes::Bytes;

/// A fully reassembled Ogg packet (continuation pages already merged).
///
/// `granule_position`/`bos`/`eos` are page-level fields (RFC 3533) attached
/// here to the page that *completed* the packet — spec-precise for the last
/// packet completed on a page. `page_index`/`page_count` record where the
/// packet sits among the packets completed on that finishing page, so a
/// codec-aware layer can back-compute each packet's own end position from the
/// page granule (the facade does this for Opus via TOC frame durations).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    /// Packet payload (continuation pages merged).
    pub data: Bytes,
    /// The finishing page's granule position (codec-defined units; e.g. sample count).
    pub granule_position: i64,
    /// Logical bitstream serial number.
    pub serial: u32,
    /// Set if the containing page was the first page of the logical stream.
    pub bos: bool,
    /// Set if the containing page was the last page of the logical stream.
    pub eos: bool,
    /// Index of this packet among the packets completed on its finishing page.
    pub page_index: u32,
    /// Total packets completed on its finishing page.
    pub page_count: u32,
}
