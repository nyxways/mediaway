//! Per-codec extradata (`avcC`/`hvcC`) extraction from a finished `CMSampleBuffer`'s
//! `CMFormatDescription`, reusing `iso_bmff::bitstream::{avc,hevc}` box builders rather than
//! writing new ones — mirrors [`super::video::extract_avcc_extra_data`]'s original H.264-only
//! shape, generalized to also cover HEVC's 3-parameter-set (VPS+SPS+PPS) record.
#![allow(unsafe_code)] // real `objc2-*` FFI calls — see `apple/mod.rs`'s doc comment

use mediaway_common::Bytes;

use objc2_core_media::{
    CMFormatDescription, CMVideoFormatDescriptionGetH264ParameterSetAtIndex,
    CMVideoFormatDescriptionGetHEVCParameterSetAtIndex,
};

const NO_ERROR: i32 = 0;

/// SPS/PPS → `avcC`, reusing `iso_bmff::bitstream::avc::to_avcc`.
pub(super) fn extract_h264(format_desc: &CMFormatDescription) -> Option<Bytes> {
    let (sps_ptr, sps_len, param_count) = h264_parameter_set_at_index(format_desc, 0)?;
    if param_count < 2 {
        return None;
    }
    let (pps_ptr, pps_len, _) = h264_parameter_set_at_index(format_desc, 1)?;

    // SAFETY: VideoToolbox guarantees `sps_ptr`/`pps_ptr` point at `sps_len`/`pps_len` valid
    // bytes of `format_desc`'s own internal memory for as long as `format_desc` is retained (per
    // `CMVideoFormatDescriptionGetH264ParameterSetAtIndex`'s own doc comment); copied out
    // immediately below, no pointer outlives this function.
    let (sps, pps) = unsafe {
        (
            std::slice::from_raw_parts(sps_ptr, sps_len),
            std::slice::from_raw_parts(pps_ptr, pps_len),
        )
    };

    let mut annex_b = Vec::with_capacity(8 + sps.len() + pps.len());
    annex_b.extend_from_slice(&[0, 0, 0, 1]);
    annex_b.extend_from_slice(sps);
    annex_b.extend_from_slice(&[0, 0, 0, 1]);
    annex_b.extend_from_slice(pps);

    iso_bmff::bitstream::avc::to_avcc(&annex_b).avcc
}

/// VPS/SPS/PPS → `hvcC`, reusing `iso_bmff::bitstream::hevc::to_hvcc` — the HEVC analog of
/// [`extract_h264`].
pub(super) fn extract_hevc(format_desc: &CMFormatDescription) -> Option<Bytes> {
    let (vps_ptr, vps_len, param_count) = hevc_parameter_set_at_index(format_desc, 0)?;
    if param_count < 3 {
        return None;
    }
    let (sps_ptr, sps_len, _) = hevc_parameter_set_at_index(format_desc, 1)?;
    let (pps_ptr, pps_len, _) = hevc_parameter_set_at_index(format_desc, 2)?;

    // SAFETY: same reasoning as `extract_h264` above, for
    // `CMVideoFormatDescriptionGetHEVCParameterSetAtIndex`'s identical "points at internal
    // memory of `format_desc`, valid while retained" contract.
    let (vps, sps, pps) = unsafe {
        (
            std::slice::from_raw_parts(vps_ptr, vps_len),
            std::slice::from_raw_parts(sps_ptr, sps_len),
            std::slice::from_raw_parts(pps_ptr, pps_len),
        )
    };

    let mut annex_b = Vec::with_capacity(12 + vps.len() + sps.len() + pps.len());
    for nal in [vps, sps, pps] {
        annex_b.extend_from_slice(&[0, 0, 0, 1]);
        annex_b.extend_from_slice(nal);
    }

    iso_bmff::bitstream::hevc::to_hvcc(&annex_b).hvcc
}

/// One parameter-set NAL unit (pointer + length, into `format_desc`'s own internal memory) plus
/// the total parameter-set count in `format_desc`'s AVC decoder configuration record.
fn h264_parameter_set_at_index(
    format_desc: &CMFormatDescription,
    index: usize,
) -> Option<(*const u8, usize, usize)> {
    let mut ptr: *const u8 = std::ptr::null();
    let mut len: usize = 0;
    let mut count: usize = 0;

    // SAFETY: `format_desc` is a valid, retained `CMFormatDescription` (obtained from the
    // callback's sample buffer); all out-pointers below are valid local stack slots.
    let status = unsafe {
        CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
            format_desc,
            index,
            &raw mut ptr,
            &raw mut len,
            &raw mut count,
            std::ptr::null_mut(),
        )
    };
    if status != NO_ERROR || ptr.is_null() {
        return None;
    }
    Some((ptr, len, count))
}

/// One parameter-set NAL unit (pointer + length, into `format_desc`'s own internal memory) plus
/// the total parameter-set count in `format_desc`'s HEVC decoder configuration record — the
/// HEVC analog of [`h264_parameter_set_at_index`].
fn hevc_parameter_set_at_index(
    format_desc: &CMFormatDescription,
    index: usize,
) -> Option<(*const u8, usize, usize)> {
    let mut ptr: *const u8 = std::ptr::null();
    let mut len: usize = 0;
    let mut count: usize = 0;

    // SAFETY: same reasoning as `h264_parameter_set_at_index` above.
    let status = unsafe {
        CMVideoFormatDescriptionGetHEVCParameterSetAtIndex(
            format_desc,
            index,
            &raw mut ptr,
            &raw mut len,
            &raw mut count,
            std::ptr::null_mut(),
        )
    };
    if status != NO_ERROR || ptr.is_null() {
        return None;
    }
    Some((ptr, len, count))
}
