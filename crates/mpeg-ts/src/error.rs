//! Public error type.

#![forbid(unsafe_code)]

/// Errors from MPEG-TS mux/demux.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A PID is outside the 13-bit range (`> 0x1FFF`) or uses a reserved value (`0` = PAT, `1` = CAT).
    #[error("PID {0} is out of range or reserved (PAT=0, CAT=1; max 0x1FFF)")]
    InvalidPid(u16),
    /// `AccessUnit::pid` (or `Muxer::write_access_unit`'s `pid` argument) is not
    /// one of the streams registered in the PMT.
    #[error("PID {0} is not a registered elementary stream")]
    UnknownPid(u16),
    /// The first byte of a TS packet is not `0x47`.
    #[error("bad TS sync byte (expected 0x47)")]
    BadSyncByte,
    /// A PSI section's CRC-32 doesn't match the computed value.
    #[error("PSI section CRC mismatch: header claims {expected:#010x}, computed {computed:#010x}")]
    CrcMismatch {
        /// CRC from the section's trailing field.
        expected: u32,
        /// CRC recomputed over the section bytes.
        computed: u32,
    },
    /// PAT/PMT `table_id` byte doesn't match the expected value for that table.
    #[error("unexpected PSI table_id {actual} (expected {expected})")]
    UnexpectedTableId {
        /// Expected `table_id`.
        expected: u8,
        /// Actual `table_id` read.
        actual: u8,
    },
    /// PES packet's `start_code_prefix` isn't `0x000001`.
    #[error("bad PES start code prefix")]
    BadPesStartCode,
    /// PMT references a `stream_type` byte this crate doesn't recognize (see [`crate::StreamType`]).
    #[error("unrecognized PMT stream_type {0}")]
    UnrecognizedStreamType(u8),
}
