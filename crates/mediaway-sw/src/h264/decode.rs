//! Top-level H.264 Baseline/CAVLC/I-slice single-frame decode entry point.
//!
//! [`decode_i_frame`] drives one slice NAL unit's `slice_header()` + macroblock loop to a
//! complete reconstructed picture. See `adr/0003-cavlc-i-slice-first-decode.md` for the
//! exact scope this covers (and does not): Baseline profile, CAVLC only, I-slices only,
//! `I_16x16`/`I_PCM` macroblocks only (`I_NxN` rejected), 4:2:0 only, **no deblocking
//! filter** (a real, visible quality gap — see that ADR's "Consequences" section).
//!
//! **Cropping caveat:** this decode loop only trims the bottom/right of the reconstructed
//! macroblock grid down to [`Sps::width`]/[`Sps::height`] (i.e. assumes `crop_left ==
//! crop_top == 0`). It does not track the individual `frame_cropping` offsets needed for a
//! true top/left-anchored crop — the common case (removing macroblock-alignment padding
//! from the bottom/right, e.g. 1080p from a 1088-tall macroblock grid) decodes correctly;
//! a stream that crops from the top or left would not.

#![forbid(unsafe_code)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "picture/plane dimensions below come from SPS macroblock counts, already \
              validated to fit usize by the checked conversions at the top of this module"
)]

use super::bitreader::BitReader;
use super::error::H264Error;
use super::nal::NalUnit;
use super::pps::Pps;
use super::reconstruct::{self, McbContext, Picture};
use super::slice::SliceHeader;
use super::sps::Sps;
use mediaway_common::{Bytes, PixelFormat, VideoFrame, VideoFrameStorage};

/// Decode one I-slice NAL unit into a complete [`VideoFrame`].
///
/// `slice_nal.unit_type` must be [`super::NalUnitType::IdrSlice`] or
/// [`super::NalUnitType::NonIdrSlice`]; the result is always [`PixelFormat::I420`] CPU
/// storage — this crate has no GPU device.
///
/// `sps`/`pps` must be the parameter sets the slice refers to (`slice_header()`'s
/// `pic_parameter_set_id` and the PPS's own `seq_parameter_set_id` are not cross-checked
/// against a caller-held table here — callers own PPS/SPS id-based lookup, matching this
/// crate's sans-io scope of transforming bytes already in memory).
///
/// # Errors
///
/// - [`H264Error::UnsupportedEntropyCoding`] if `pps.entropy_coding_mode` selects CABAC.
/// - [`H264Error::UnsupportedChromaFormat`] if `sps.chroma_format_idc != 1` (4:2:0).
/// - [`H264Error::UnsupportedSliceType`] / [`H264Error::UnsupportedPicOrderCntType`] /
///   [`H264Error::UnsupportedFieldCoding`] from [`SliceHeader::parse`].
/// - [`H264Error::MultiSliceUnsupported`] if `first_mb_in_slice != 0` — this decode loop
///   only supports one slice covering the entire picture.
/// - [`H264Error::UnsupportedMbType`] on an `I_NxN` macroblock (see module docs).
/// - Other [`H264Error`] variants from malformed/truncated macroblock or CAVLC data.
pub fn decode_i_frame(sps: &Sps, pps: &Pps, slice_nal: &NalUnit) -> Result<VideoFrame, H264Error> {
    if pps.entropy_coding_mode {
        return Err(H264Error::UnsupportedEntropyCoding);
    }
    if sps.chroma_format_idc != 1 {
        return Err(H264Error::UnsupportedChromaFormat);
    }

    let mut reader = BitReader::new(&slice_nal.rbsp);
    let header = SliceHeader::parse(
        &mut reader,
        sps,
        pps,
        slice_nal.unit_type,
        slice_nal.ref_idc,
    )?;
    if header.first_mb_in_slice != 0 {
        return Err(H264Error::MultiSliceUnsupported);
    }

    let mb_width =
        usize::try_from(sps.pic_width_in_mbs).map_err(|_err| H264Error::FieldOverflow)?;
    let mb_height =
        usize::try_from(sps.pic_height_in_mbs).map_err(|_err| H264Error::FieldOverflow)?;
    let num_mbs = mb_width
        .checked_mul(mb_height)
        .ok_or(H264Error::FieldOverflow)?;

    let mut picture = Picture::new(mb_width, mb_height);
    let mut ctx = McbContext::new(num_mbs, mb_width);
    // ITU-T H.264 § 7.4.5: SliceQPY = 26 + pic_init_qp_minus26 + slice_qp_delta, used as
    // QPy,prev for the first macroblock in the slice.
    let mut qp_prev = pps
        .pic_init_qp
        .checked_add(header.slice_qp_delta)
        .ok_or(H264Error::FieldOverflow)?;

    for mb_addr in 0..num_mbs {
        reconstruct::decode_macroblock(
            &mut reader,
            pps,
            mb_addr,
            &mut qp_prev,
            &mut picture,
            &mut ctx,
        )?;
    }

    pack_video_frame(sps, &picture)
}

/// Crop the reconstructed macroblock-grid-sized planes down to `sps.width`/`sps.height`
/// (see module docs on the top/left-anchored cropping caveat) and pack them into a tightly
/// packed I420 [`VideoFrame`] (`Y` plane, then `U`, then `V` — no row padding, the same
/// layout `mediaway-sw`'s own `av1` module reads on the encode side).
fn pack_video_frame(sps: &Sps, picture: &Picture) -> Result<VideoFrame, H264Error> {
    let width = usize::try_from(sps.width).map_err(|_err| H264Error::FieldOverflow)?;
    let height = usize::try_from(sps.height).map_err(|_err| H264Error::FieldOverflow)?;
    // Defensive: the cropped SPS dimensions must fit inside the reconstructed macroblock
    // grid (this only fails if `Sps::parse`'s own crop arithmetic had a bug, since it
    // already derives width/height by subtracting from the raw macroblock-grid size).
    if width > picture.mb_width * 16 || height > picture.mb_height * 16 {
        return Err(H264Error::FieldOverflow);
    }
    let chroma_width = width.div_ceil(2);
    let chroma_height = height.div_ceil(2);

    let mut data = Vec::with_capacity(width * height + 2 * chroma_width * chroma_height);
    crop_plane_into(&picture.y, picture.y_stride, width, height, &mut data)?;
    crop_plane_into(
        &picture.u,
        picture.c_stride,
        chroma_width,
        chroma_height,
        &mut data,
    )?;
    crop_plane_into(
        &picture.v,
        picture.c_stride,
        chroma_width,
        chroma_height,
        &mut data,
    )?;

    Ok(VideoFrame {
        pts: 0,
        duration: 0,
        width: sps.width,
        height: sps.height,
        format: PixelFormat::I420,
        storage: VideoFrameStorage::Cpu {
            data: Bytes::from(data),
        },
    })
}

/// Append the top-left `width x height` region of `plane` (row stride `stride`) to `out`.
fn crop_plane_into(
    plane: &[u8],
    stride: usize,
    width: usize,
    height: usize,
    out: &mut Vec<u8>,
) -> Result<(), H264Error> {
    for row in 0..height {
        let start = row.checked_mul(stride).ok_or(H264Error::FieldOverflow)?;
        let end = start.checked_add(width).ok_or(H264Error::FieldOverflow)?;
        out.extend_from_slice(plane.get(start..end).ok_or(H264Error::FieldOverflow)?);
    }
    Ok(())
}

#[cfg(test)]
#[path = "decode_tests.rs"]
mod tests;
