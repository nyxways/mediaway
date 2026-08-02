//! Freestanding Ogg packet type (no Mediaway dependency).

#![forbid(unsafe_code)]

use bytes::Bytes;

/// A fully reassembled Ogg packet (continuation pages already merged).
///
/// `granule_position`/`bos`/`eos` are page-level fields (RFC 3533) attached here
/// to every packet completed while parsing that page — spec-precise only for the
/// *last* packet completed on a page; earlier packets on the same page share the
/// same values as an approximation (documented in crate-local ADR-0001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    /// Packet payload (continuation pages merged).
    pub data: Bytes,
    /// The page's granule position (codec-defined units; e.g. sample count).
    pub granule_position: i64,
    /// Logical bitstream serial number.
    pub serial: u32,
    /// Set if the containing page was the first page of the logical stream.
    pub bos: bool,
    /// Set if the containing page was the last page of the logical stream.
    pub eos: bool,
}
