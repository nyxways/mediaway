//! Errors for H.264 bitstream framing and SPS/PPS header parsing.

#![forbid(unsafe_code)]

use thiserror::Error;

/// Errors from parsing H.264 Annex-B/AVCC NAL units or SPS/PPS headers.
///
/// All variants come from untrusted input bytes — none of these are ever raised for
/// programmer error, so callers should treat them as ordinary rejected-input results.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum H264Error {
    /// Input ended before an expected byte or bit field could be read.
    #[error("truncated H.264 bitstream data")]
    UnexpectedEof,
    /// No Annex-B start code (`0x000001` / `0x00000001`) found in the input.
    #[error("no Annex-B start code found")]
    NoStartCode,
    /// An AVCC length-prefixed NAL unit's declared length exceeds the remaining buffer.
    #[error("AVCC NAL unit length exceeds remaining data")]
    InvalidNalLength,
    /// AVCC NAL unit length-size field must be `1..=4` bytes.
    #[error("invalid AVCC NAL length size")]
    InvalidLengthSize,
    /// An Exp-Golomb (`ue(v)`/`se(v)`) code decoded to a value outside the representable range.
    #[error("exponential-Golomb value out of range")]
    ExpGolombOverflow,
    /// A field computed from parsed values (e.g. cropped width/height, `+1` offsets)
    /// overflowed its target integer type.
    #[error("parsed field value overflowed its target range")]
    FieldOverflow,
    /// SPS `chroma_format_idc` decoded outside the spec-defined `0..=3` range.
    #[error("invalid SPS chroma_format_idc value")]
    InvalidChromaFormat,
    /// PPS declared more than one slice group (`num_slice_groups_minus1 > 0`, FMO/ASO),
    /// which this parser does not decode.
    #[error("PPS slice groups other than one are not supported")]
    SliceGroupsUnsupported,
    /// `entropy_coding_mode_flag` selects CABAC; only CAVLC is decoded
    /// (see `adr/0003-cavlc-i-slice-first-decode.md`).
    #[error("CABAC entropy coding is not supported (CAVLC only)")]
    UnsupportedEntropyCoding,
    /// Slice type is P/B/SP/SI; only I-slices are decoded.
    #[error("only I-slices are supported for pixel decode")]
    UnsupportedSliceType,
    /// `pic_order_cnt_type != 0`; only type 0 is parsed.
    #[error("only pic_order_cnt_type 0 is supported")]
    UnsupportedPicOrderCntType,
    /// `frame_mbs_only_flag == 0` (field/MBAFF pictures); only frame pictures are decoded.
    #[error("field-coded pictures are not supported")]
    UnsupportedFieldCoding,
    /// `chroma_format_idc != 1`; only 4:2:0 is decoded.
    #[error("only 4:2:0 chroma format is supported")]
    UnsupportedChromaFormat,
    /// `first_mb_in_slice != 0`, or a slice ran out of `more_rbsp_data()` before covering
    /// every macroblock in the picture — multi-slice pictures are not composed.
    #[error("multi-slice pictures are not supported (one slice must cover the whole picture)")]
    MultiSliceUnsupported,
    /// `mb_type` decoded to a value outside the valid I-slice range (`0..=25`).
    #[error("invalid I-slice mb_type value")]
    InvalidMbType,
    /// `mb_type` selects `I_NxN` (4x4/8x8) intra prediction, which this decode loop does not
    /// reconstruct yet (see `adr/0003-cavlc-i-slice-first-decode.md`); only `I_16x16` and
    /// `I_PCM` macroblocks are reconstructed.
    #[error("I_NxN macroblock reconstruction is not supported yet")]
    UnsupportedMbType,
    /// A CAVLC variable-length code did not match any codeword in the applicable table —
    /// malformed or truncated residual data.
    #[error("invalid CAVLC codeword")]
    InvalidCavlcCode,
    /// An intra prediction mode (Vertical/Horizontal/Plane) required a neighbouring
    /// macroblock that decode-loop bookkeeping marked unavailable — only reachable from a
    /// non-conformant bitstream (a real encoder never signals a mode whose required
    /// neighbours do not exist).
    #[error("intra prediction mode requires an unavailable neighbouring macroblock")]
    UnavailableIntraNeighbor,
}
