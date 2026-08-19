//! Per-codec `CMFormatDescription` construction for `VTDecompressionSession`.
//!
//! H.264/HEVC build via their dedicated `CMVideoFormatDescriptionCreateFrom{H264,HEVC}
//! ParameterSets` entry points — VideoToolbox parses geometry out of the parameter sets itself,
//! same lazy-first-packet-discovery shape [`super::video`] already used for H.264 alone. VP9/AV1
//! have no such entry point: VideoToolbox only exposes the generic
//! `CMVideoFormatDescriptionCreate(codecType, width, height, extensions)` for them, so this
//! backend requires the container to supply a `vpcC`/`av1C` config record up front
//! ([`super::codec::requires_extra_data_at_open`]) and wraps it verbatim as a
//! `SampleDescriptionExtensionAtoms` extension atom — there is no per-frame parameter-set NAL to
//! discover from either codec's bitstream the way H.264/HEVC's VPS/SPS/PPS can be, and this
//! backend does not carry a VP9/AV1 bitstream parser of its own to synthesize one (unlike this
//! workspace's VA-API/Vulkan/D3D12 decoders, which parse full picture parameters because their
//! session APIs need them — VideoToolbox is a black box that only needs enough to pick a codec
//! path).
#![allow(unsafe_code)] // real `objc2-*` FFI calls — see `apple/mod.rs`'s doc comment

use std::ptr::NonNull;

use bytes::Bytes;

use crate::DecodeError;
use mediaway_common::CodecKind;

use objc2_core_foundation::{CFData, CFDictionary, CFPropertyList, CFRetained, CFString, CFType};
use objc2_core_media::{
    CMFormatDescription, CMVideoCodecType, CMVideoFormatDescription,
    kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms, kCMVideoCodecType_AV1,
    kCMVideoCodecType_VP9,
};

const NO_ERROR: i32 = 0;

/// `VideoToolbox` codec type for a raw (non-NAL) codec this backend supports —
/// [`CodecKind::Vp9`]/[`CodecKind::Av1`] only; H.264/HEVC build their format description via
/// dedicated parameter-set entry points instead (see [`create_h264`]/[`create_hevc`]).
#[must_use]
pub(super) const fn raw_codec_type(codec: CodecKind) -> Option<CMVideoCodecType> {
    match codec {
        CodecKind::Vp9 => Some(kCMVideoCodecType_VP9),
        CodecKind::Av1 => Some(kCMVideoCodecType_AV1),
        _ => None,
    }
}

/// SPS/PPS → `CMFormatDescription`, via `CMVideoFormatDescriptionCreateFromH264ParameterSets`.
/// `sps`/`pps` must be raw NAL payload with any emulation prevention bytes needed and no start
/// code / length prefix — exactly what `iso_bmff::bitstream::avc::AvcDecoderConfig::{sps, pps}`
/// already returns.
pub(super) fn create_h264(
    sps: &Bytes,
    pps: &Bytes,
) -> Result<CFRetained<CMVideoFormatDescription>, DecodeError> {
    if sps.is_empty() || pps.is_empty() {
        return Err(DecodeError::InvalidInput);
    }
    let Some(sps_ptr) = NonNull::new(sps.as_ptr().cast_mut()) else {
        return Err(DecodeError::InvalidInput);
    };
    let Some(pps_ptr) = NonNull::new(pps.as_ptr().cast_mut()) else {
        return Err(DecodeError::InvalidInput);
    };
    let mut pointers = [sps_ptr, pps_ptr];
    let mut sizes = [sps.len(), pps.len()];
    let Some(pointers_ptr) = NonNull::new(pointers.as_mut_ptr()) else {
        return Err(DecodeError::Backend);
    };
    let Some(sizes_ptr) = NonNull::new(sizes.as_mut_ptr()) else {
        return Err(DecodeError::Backend);
    };

    let mut format_desc_out: Option<CFRetained<CMFormatDescription>> = None;
    // SAFETY: `pointers_ptr`/`sizes_ptr` point at 2-element stack arrays matching
    // `parameter_set_count = 2`; each pointer in `pointers` is valid for the corresponding
    // `sizes` entry's byte length for the duration of this call (borrowed from `sps`/`pps`,
    // both live for this whole function); `4` (`nal_unit_header_length`) matches this backend's
    // 4-byte AVCC length-prefix scope; `format_desc_out` starts `None`.
    let status = unsafe {
        CMVideoFormatDescription::from_h264_parameter_sets(
            None,
            2,
            pointers_ptr,
            sizes_ptr,
            4,
            &mut format_desc_out,
        )
    };
    if status != NO_ERROR {
        return Err(DecodeError::Backend);
    }
    let format_desc = format_desc_out.ok_or(DecodeError::Backend)?;
    // SAFETY: `format_desc` was just created by
    // `CMVideoFormatDescriptionCreateFromH264ParameterSets`, which per its own doc comment only
    // ever produces a format description describing H.264 video — casting to the video-specific
    // view type is exactly this API's documented purpose (`CMVideoFormatDescription` shares
    // `CMFormatDescription`'s `CFTypeID`; it is not a distinct concrete type with its own
    // `ConcreteType::type_id`).
    Ok(unsafe { CFRetained::cast_unchecked::<CMVideoFormatDescription>(format_desc) })
}

/// VPS/SPS/PPS → `CMFormatDescription`, via
/// `CMVideoFormatDescriptionCreateFromHEVCParameterSets`. `vps`/`sps`/`pps` must be raw NAL
/// payload with any emulation prevention bytes needed and no start code / length prefix —
/// exactly what `iso_bmff::bitstream::hevc::HevcDecoderConfig::{vps, sps, pps}` already returns.
pub(super) fn create_hevc(
    vps: &Bytes,
    sps: &Bytes,
    pps: &Bytes,
) -> Result<CFRetained<CMVideoFormatDescription>, DecodeError> {
    if vps.is_empty() || sps.is_empty() || pps.is_empty() {
        return Err(DecodeError::InvalidInput);
    }
    let Some(vps_ptr) = NonNull::new(vps.as_ptr().cast_mut()) else {
        return Err(DecodeError::InvalidInput);
    };
    let Some(sps_ptr) = NonNull::new(sps.as_ptr().cast_mut()) else {
        return Err(DecodeError::InvalidInput);
    };
    let Some(pps_ptr) = NonNull::new(pps.as_ptr().cast_mut()) else {
        return Err(DecodeError::InvalidInput);
    };
    let mut pointers = [vps_ptr, sps_ptr, pps_ptr];
    let mut sizes = [vps.len(), sps.len(), pps.len()];
    let Some(pointers_ptr) = NonNull::new(pointers.as_mut_ptr()) else {
        return Err(DecodeError::Backend);
    };
    let Some(sizes_ptr) = NonNull::new(sizes.as_mut_ptr()) else {
        return Err(DecodeError::Backend);
    };

    let mut format_desc_out: Option<CFRetained<CMFormatDescription>> = None;
    // SAFETY: `pointers_ptr`/`sizes_ptr` point at 3-element stack arrays matching
    // `parameter_set_count = 3`; each pointer in `pointers` is valid for the corresponding
    // `sizes` entry's byte length for the duration of this call (borrowed from `vps`/`sps`/
    // `pps`, all live for this whole function); `4` (`nal_unit_header_length`) matches this
    // backend's 4-byte `hvcC` length-prefix scope; `extensions: None` (nothing to add beyond
    // the parameter sets themselves); `format_desc_out` starts `None`.
    let status = unsafe {
        CMVideoFormatDescription::from_hevc_parameter_sets(
            None,
            3,
            pointers_ptr,
            sizes_ptr,
            4,
            None,
            &mut format_desc_out,
        )
    };
    if status != NO_ERROR {
        return Err(DecodeError::Backend);
    }
    let format_desc = format_desc_out.ok_or(DecodeError::Backend)?;
    // SAFETY: same reasoning as `create_h264`'s own cast, for HEVC.
    Ok(unsafe { CFRetained::cast_unchecked::<CMVideoFormatDescription>(format_desc) })
}

/// `width`/`height` + a raw container-supplied codec-config atom (`vpcC` for VP9, `av1C` for
/// AV1) → `CMFormatDescription`, via the generic `CMVideoFormatDescriptionCreate` plus a
/// `SampleDescriptionExtensionAtoms` extension dictionary — VideoToolbox's only construction
/// path for either codec (see this module's doc comment). `atom_payload` is wrapped byte-for-
/// byte, unparsed — this backend trusts the container's config record rather than re-deriving it
/// from the bitstream.
pub(super) fn create_raw(
    codec_type: CMVideoCodecType,
    width: i32,
    height: i32,
    atom_key: &'static str,
    atom_payload: &[u8],
) -> Result<CFRetained<CMVideoFormatDescription>, DecodeError> {
    if atom_payload.is_empty() {
        return Err(DecodeError::InvalidInput);
    }
    let atom_data = CFData::from_bytes(atom_payload);
    let atom_data_ct: &CFType = &atom_data;
    let atom_key_cf = CFString::from_static_str(atom_key);
    let atom_key_ref: &CFString = &atom_key_cf;
    let inner = CFDictionary::<CFString, CFType>::from_slices(&[atom_key_ref], &[atom_data_ct]);
    let inner_ct: &CFType = &inner;

    // SAFETY: `kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms` is a real,
    // always-initialized `extern "C"` framework constant, safe to read for the process's
    // lifetime — same pattern as this backend's other static CF constant reads.
    let outer_key = unsafe { kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms };
    let outer = CFDictionary::<CFString, CFType>::from_slices(&[outer_key], &[inner_ct]);
    // SAFETY: `CFDictionary<CFString, CFType>` and `CFDictionary<CFString, CFPropertyList>` are
    // the identical Core Foundation object at runtime — `CFType`/`CFPropertyList` are both
    // phantom marker types for "any CF property-list-compatible value" (CFDictionary/CFData/
    // CFString/CFNumber/… all conform to both), the same toll-free-bridging assumption
    // `CMVideoFormatDescription::from_hevc_parameter_sets`'s own `extensions` parameter already
    // makes; only the static phantom type changes here, not the underlying object.
    let outer: CFRetained<CFDictionary<CFString, CFPropertyList>> =
        unsafe { CFRetained::cast_unchecked(outer) };

    let mut format_desc_out: Option<CFRetained<CMVideoFormatDescription>> = None;
    // SAFETY: `codec_type` is a real, VideoToolbox-recognized `CMVideoCodecType`; `width`/
    // `height` are positive (checked by this backend's `validate` before this is called);
    // `outer` is a valid, just-built extensions dictionary; `format_desc_out` starts `None`.
    let status = unsafe {
        CMVideoFormatDescription::new(
            None,
            codec_type,
            width,
            height,
            Some(&outer),
            &mut format_desc_out,
        )
    };
    if status != NO_ERROR {
        return Err(DecodeError::Backend);
    }
    format_desc_out.ok_or(DecodeError::Backend)
}
