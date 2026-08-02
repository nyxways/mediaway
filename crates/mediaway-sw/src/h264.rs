//! Pure-Rust H.264 Annex-B/AVCC NAL unit, SPS/PPS header parsing, and Baseline/CAVLC/
//! I-slice single-frame pixel decode.
//!
//! Sans-io: every function here operates on in-memory byte slices only — no file,
//! socket, or device IO.
//!
//! - Bitstream framing: NAL unit splitting, `emulation_prevention_three_byte` removal
//!   ([`split_annex_b`], [`split_avcc`], [`NalUnit`]).
//! - Header parsing: SPS/PPS ([`Sps`], [`Pps`]) and slice headers ([`SliceHeader`]).
//! - Pixel decode: [`decode_i_frame`] — Baseline profile, CAVLC only, I-slices only,
//!   `I_16x16`/`I_PCM` macroblocks only (`I_NxN` rejected), 4:2:0 only, **no deblocking
//!   filter**. See `adr/0001-h264-baseline-decoder-first.md` (staging) and
//!   `adr/0003-cavlc-i-slice-first-decode.md` (this decode loop's exact scope cuts).

#![forbid(unsafe_code)]

mod bitreader;
mod cavlc;
mod cavlc_tables;
mod decode;
mod error;
mod intra_pred;
mod macroblock;
mod nal;
mod pps;
mod reconstruct;
mod slice;
mod sps;
mod transform;

pub use bitreader::BitReader;
pub use decode::decode_i_frame;
pub use error::H264Error;
pub use macroblock::MbType;
pub use nal::{NalUnit, NalUnitType, split_annex_b, split_avcc};
pub use pps::Pps;
pub use slice::{SliceHeader, SliceType};
pub use sps::Sps;
