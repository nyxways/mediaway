//! `open`-time D3D12 object creation for AV1 — mirrors [`super::hevc`]'s HEVC feature
//! queries and `ID3D12VideoEncoder`/`ID3D12VideoEncoderHeap` creation, but for
//! `D3D12_VIDEO_ENCODER_CODEC_AV1` / `D3D12_VIDEO_ENCODER_AV1_PROFILE_MAIN`. Kept as a
//! separate file for the same C-union reasons [`super::hevc`]'s doc comment explains.
//!
//! This backend declares the three AV1 coding tools this driver's `RequiredFeatureFlags`
//! mandates (`AUTO_SEGMENTATION | CDEF_FILTERING | LOOP_RESTORATION_FILTER`, see finding 2
//! below) as *available* at the session level — but every frame this backend actually
//! encodes stays a plain all-intra, no-segmentation/CDEF/restoration bitstream, matching
//! [`super::bitstream_av1`]'s sequence header, which disables every corresponding
//! `enable_*` flag. See [`default_codec_config_av1`]'s doc for why declaring a tool
//! available and actually using it per frame are independent D3D12 concepts.
//!
//! **Two distinct real findings (2026-08-06/07), not one — the earlier "driver doesn't
//! support AV1" conclusion was wrong:**
//!
//! 1. **Wrong query.** The official D3D12 AV1 encode spec states plainly that
//!    `D3D12_FEATURE_VIDEO_ENCODER_SUPPORT` — what this file originally called — "will not
//!    work for AV1 codec as *Codec* input": it still returns `S_OK`, but with
//!    `ValidationFlags = D3D12_VIDEO_ENCODER_VALIDATION_FLAG_CODEC_NOT_SUPPORTED`, and a
//!    debug-layer message pointing at the newer query instead. That `CODEC_NOT_SUPPORTED`
//!    result is exactly what this backend previously (wrongly) read as "this driver has no
//!    AV1 encode." [`check_encoder_support`] below now queries the AV1-capable replacement,
//!    `D3D12_FEATURE_VIDEO_ENCODER_SUPPORT1` (`D3D12_FEATURE_DATA_VIDEO_ENCODER_SUPPORT1`,
//!    a strict superset of the old struct with two new trailing fields —
//!    `SubregionFrameEncodingData`, `MaxQualityVsSpeed`).
//! 2. **Wrong codec configuration.** With the query fixed, `CheckFeatureSupport` on the
//!    RTX 4090 this crate is verified against now reports
//!    `ValidationFlags = CODEC_CONFIGURATION_NOT_SUPPORTED` instead — i.e. the codec itself
//!    is recognized (confirmed separately via `D3D12_FEATURE_VIDEO_ENCODER_CODEC`,
//!    `IsSupported == true`), but this file's all-`NONE` [`default_codec_config_av1`] is
//!    rejected. `D3D12_FEATURE_VIDEO_ENCODER_CODEC_CONFIGURATION_SUPPORT`
//!    (`pAV1Support: D3D12_VIDEO_ENCODER_AV1_CODEC_CONFIGURATION_SUPPORT`) on this same
//!    hardware reports `RequiredFeatureFlags = AUTO_SEGMENTATION | CDEF_FILTERING |
//!    LOOP_RESTORATION_FILTER` (`0x11400`) — this driver mandates those three AV1 coding
//!    tools be declared *available* at the session level (`FEATURE_FLAG_NONE` is not a
//!    legal configuration here); [`default_codec_config_av1`] now declares them. This is a
//!    **session-level capability declaration only** — it does not force any single frame to
//!    actually use segmentation/CDEF/restoration. `ENABLE_FRAME_SEGMENTATION_AUTO`/`_CUSTOM`
//!    are separate, optional **per-frame** flags (spec § 4.1.1), and `enable_cdef 0`/
//!    `enable_restoration 0` in our own hand-written sequence header (§ 5.5.1) make
//!    `cdef_params()`/`lr_params()` read zero bits regardless of what the session declared
//!    supported — so this backend's real per-frame bitstream stays exactly as conservative
//!    as before this change.
//!
//! See `local/standards/d3d12-video-encoding-av1/d3d12_video_encoding_av1.md` §§ 3.1.24,
//! 3.1.38 for both structs.

use crate::EncodeError;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_NV12;
use windows::Win32::Graphics::Dxgi::Common::DXGI_RATIONAL;
use windows::Win32::Media::MediaFoundation::{
    D3D12_FEATURE_DATA_VIDEO_ENCODER_RESOLUTION_SUPPORT_LIMITS,
    D3D12_FEATURE_DATA_VIDEO_ENCODER_RESOURCE_REQUIREMENTS,
    D3D12_FEATURE_DATA_VIDEO_ENCODER_SUPPORT1, D3D12_FEATURE_VIDEO_ENCODER_RESOURCE_REQUIREMENTS,
    D3D12_FEATURE_VIDEO_ENCODER_SUPPORT1, D3D12_VIDEO_ENCODER_AV1_CODEC_CONFIGURATION,
    D3D12_VIDEO_ENCODER_AV1_FEATURE_FLAG_AUTO_SEGMENTATION,
    D3D12_VIDEO_ENCODER_AV1_FEATURE_FLAG_CDEF_FILTERING,
    D3D12_VIDEO_ENCODER_AV1_FEATURE_FLAG_LOOP_RESTORATION_FILTER,
    D3D12_VIDEO_ENCODER_AV1_LEVEL_TIER_CONSTRAINTS, D3D12_VIDEO_ENCODER_AV1_PROFILE,
    D3D12_VIDEO_ENCODER_AV1_PROFILE_MAIN, D3D12_VIDEO_ENCODER_AV1_SEQUENCE_STRUCTURE,
    D3D12_VIDEO_ENCODER_CODEC_AV1, D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION,
    D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_0, D3D12_VIDEO_ENCODER_DESC,
    D3D12_VIDEO_ENCODER_FLAG_NONE, D3D12_VIDEO_ENCODER_FRAME_SUBREGION_LAYOUT_MODE_FULL_FRAME,
    D3D12_VIDEO_ENCODER_HEAP_DESC, D3D12_VIDEO_ENCODER_HEAP_FLAG_NONE,
    D3D12_VIDEO_ENCODER_INTRA_REFRESH_MODE_NONE, D3D12_VIDEO_ENCODER_LEVEL_SETTING,
    D3D12_VIDEO_ENCODER_LEVEL_SETTING_0,
    D3D12_VIDEO_ENCODER_MOTION_ESTIMATION_PRECISION_MODE_MAXIMUM,
    D3D12_VIDEO_ENCODER_PICTURE_CONTROL_SUBREGIONS_LAYOUT_DATA,
    D3D12_VIDEO_ENCODER_PICTURE_RESOLUTION_DESC, D3D12_VIDEO_ENCODER_PROFILE_DESC,
    D3D12_VIDEO_ENCODER_PROFILE_DESC_0, D3D12_VIDEO_ENCODER_RATE_CONTROL,
    D3D12_VIDEO_ENCODER_RATE_CONTROL_CONFIGURATION_PARAMS,
    D3D12_VIDEO_ENCODER_RATE_CONTROL_CONFIGURATION_PARAMS_0, D3D12_VIDEO_ENCODER_RATE_CONTROL_CQP,
    D3D12_VIDEO_ENCODER_RATE_CONTROL_FLAG_NONE, D3D12_VIDEO_ENCODER_RATE_CONTROL_MODE_CQP,
    D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE, D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_0,
    D3D12_VIDEO_ENCODER_SUPPORT_FLAG_GENERAL_SUPPORT_OK, D3D12_VIDEO_ENCODER_SUPPORT_FLAGS,
    D3D12_VIDEO_ENCODER_VALIDATION_FLAGS, ID3D12VideoDevice3, ID3D12VideoEncoder,
    ID3D12VideoEncoderHeap,
};
use windows::core::BOOL;

use super::setup::{
    check_codec_support as check_codec_support_generic,
    check_output_resolution as check_output_resolution_generic,
};
use super::util::data_size;

pub(super) fn check_codec_support(video_device: &ID3D12VideoDevice3) -> Result<(), EncodeError> {
    check_codec_support_generic(video_device, D3D12_VIDEO_ENCODER_CODEC_AV1)
}

pub(super) fn check_output_resolution(
    video_device: &ID3D12VideoDevice3,
    resolution: D3D12_VIDEO_ENCODER_PICTURE_RESOLUTION_DESC,
) -> Result<(), EncodeError> {
    check_output_resolution_generic(video_device, D3D12_VIDEO_ENCODER_CODEC_AV1, resolution)
}

fn profile_desc_main(
    profile_av1: &mut D3D12_VIDEO_ENCODER_AV1_PROFILE,
) -> D3D12_VIDEO_ENCODER_PROFILE_DESC {
    D3D12_VIDEO_ENCODER_PROFILE_DESC {
        DataSize: data_size::<D3D12_VIDEO_ENCODER_AV1_PROFILE>(),
        Anonymous: D3D12_VIDEO_ENCODER_PROFILE_DESC_0 {
            pAV1Profile: profile_av1,
        },
    }
}

pub(super) fn check_resource_requirements(
    video_device: &ID3D12VideoDevice3,
    resolution: D3D12_VIDEO_ENCODER_PICTURE_RESOLUTION_DESC,
) -> Result<D3D12_FEATURE_DATA_VIDEO_ENCODER_RESOURCE_REQUIREMENTS, EncodeError> {
    let mut profile_av1 = D3D12_VIDEO_ENCODER_AV1_PROFILE_MAIN;
    let mut req = D3D12_FEATURE_DATA_VIDEO_ENCODER_RESOURCE_REQUIREMENTS {
        NodeIndex: 0,
        Codec: D3D12_VIDEO_ENCODER_CODEC_AV1,
        Profile: profile_desc_main(&mut profile_av1),
        InputFormat: DXGI_FORMAT_NV12,
        PictureTargetResolution: resolution,
        IsSupported: BOOL::default(),
        CompressedBitstreamBufferAccessAlignment: 0,
        EncoderMetadataBufferAccessAlignment: 0,
        MaxEncoderOutputMetadataBufferSize: 0,
    };
    // SAFETY: `req` is sized/typed exactly as `D3D12_FEATURE_VIDEO_ENCODER_RESOURCE_REQUIREMENTS` expects.
    unsafe {
        video_device
            .CheckFeatureSupport(
                D3D12_FEATURE_VIDEO_ENCODER_RESOURCE_REQUIREMENTS,
                std::ptr::from_mut(&mut req).cast(),
                data_size::<D3D12_FEATURE_DATA_VIDEO_ENCODER_RESOURCE_REQUIREMENTS>(),
            )
            .map_err(|_| EncodeError::Unsupported)?;
    }
    if !req.IsSupported.as_bool() {
        return Err(EncodeError::Unsupported);
    }
    Ok(req)
}

/// AV1 codec configuration for this backend. `FeatureFlags` sets exactly the three
/// AV1 coding tools this driver's `RequiredFeatureFlags` mandates be declared at the
/// session level (`D3D12_FEATURE_VIDEO_ENCODER_CODEC_CONFIGURATION_SUPPORT` — see
/// [`super`]'s module doc, finding 2) — `AUTO_SEGMENTATION | CDEF_FILTERING |
/// LOOP_RESTORATION_FILTER`. This is a **session-level capability declaration**, not a
/// per-frame command: [`super::ops_av1`]'s per-frame `D3D12_VIDEO_ENCODER_AV1_PICTURE_
/// CONTROL_CODEC_DATA::Flags` still omits `ENABLE_FRAME_SEGMENTATION_AUTO`/`_CUSTOM` (an
/// independent, optional per-frame flag pair — spec §4.1.1), and [`super::bitstream_av1`]
/// still hand-writes `segmentation_enabled = 0`/`enable_cdef = 0`/`enable_restoration = 0`
/// in the sequence/frame headers it owns — every real AV1 stream this backend produces
/// stays a plain all-intra, no-segmentation, no-CDEF, no-restoration bitstream regardless
/// of what the session declares as *available*.
pub(super) fn default_codec_config_av1() -> D3D12_VIDEO_ENCODER_AV1_CODEC_CONFIGURATION {
    D3D12_VIDEO_ENCODER_AV1_CODEC_CONFIGURATION {
        FeatureFlags: D3D12_VIDEO_ENCODER_AV1_FEATURE_FLAG_AUTO_SEGMENTATION
            | D3D12_VIDEO_ENCODER_AV1_FEATURE_FLAG_CDEF_FILTERING
            | D3D12_VIDEO_ENCODER_AV1_FEATURE_FLAG_LOOP_RESTORATION_FILTER,
        OrderHintBitsMinus1: 0,
    }
}

/// Query `D3D12_FEATURE_VIDEO_ENCODER_SUPPORT1` (**not** the plain `_SUPPORT` query — see
/// this file's module doc for why that one always reports
/// `CODEC_NOT_SUPPORTED` for AV1) for the exact AV1 codec/GOP/rate-control/resolution
/// combination this session will use (using [`default_codec_config_av1`]'s fixed codec
/// configuration), returning the driver's `SuggestedLevel` (level **and** tier — AV1
/// levels are tier-qualified like HEVC's, unlike H.264's).
pub(super) fn check_encoder_support(
    video_device: &ID3D12VideoDevice3,
    resolution: D3D12_VIDEO_ENCODER_PICTURE_RESOLUTION_DESC,
    mut gop: D3D12_VIDEO_ENCODER_AV1_SEQUENCE_STRUCTURE,
    rc_cqp: D3D12_VIDEO_ENCODER_RATE_CONTROL_CQP,
    frame_rate: (u32, u32),
) -> Result<D3D12_VIDEO_ENCODER_AV1_LEVEL_TIER_CONSTRAINTS, EncodeError> {
    let mut codec_conf_av1 = default_codec_config_av1();
    let mut resolution_limits =
        D3D12_FEATURE_DATA_VIDEO_ENCODER_RESOLUTION_SUPPORT_LIMITS::default();
    let mut suggested_profile_av1 = D3D12_VIDEO_ENCODER_AV1_PROFILE_MAIN;
    let mut suggested_level_av1 = D3D12_VIDEO_ENCODER_AV1_LEVEL_TIER_CONSTRAINTS::default();

    let mut support = D3D12_FEATURE_DATA_VIDEO_ENCODER_SUPPORT1 {
        NodeIndex: 0,
        Codec: D3D12_VIDEO_ENCODER_CODEC_AV1,
        InputFormat: DXGI_FORMAT_NV12,
        CodecConfiguration: D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION {
            DataSize: data_size::<D3D12_VIDEO_ENCODER_AV1_CODEC_CONFIGURATION>(),
            Anonymous: D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_0 {
                pAV1Config: &raw mut codec_conf_av1,
            },
        },
        CodecGopSequence: D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE {
            DataSize: data_size::<D3D12_VIDEO_ENCODER_AV1_SEQUENCE_STRUCTURE>(),
            Anonymous: D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_0 {
                pAV1SequenceStructure: &raw mut gop,
            },
        },
        RateControl: D3D12_VIDEO_ENCODER_RATE_CONTROL {
            Mode: D3D12_VIDEO_ENCODER_RATE_CONTROL_MODE_CQP,
            Flags: D3D12_VIDEO_ENCODER_RATE_CONTROL_FLAG_NONE,
            ConfigParams: D3D12_VIDEO_ENCODER_RATE_CONTROL_CONFIGURATION_PARAMS {
                DataSize: data_size::<D3D12_VIDEO_ENCODER_RATE_CONTROL_CQP>(),
                Anonymous: D3D12_VIDEO_ENCODER_RATE_CONTROL_CONFIGURATION_PARAMS_0 {
                    pConfiguration_CQP: &raw const rc_cqp,
                },
            },
            TargetFrameRate: DXGI_RATIONAL {
                Numerator: frame_rate.0,
                Denominator: frame_rate.1,
            },
        },
        IntraRefresh: D3D12_VIDEO_ENCODER_INTRA_REFRESH_MODE_NONE,
        SubregionFrameEncoding: D3D12_VIDEO_ENCODER_FRAME_SUBREGION_LAYOUT_MODE_FULL_FRAME,
        ResolutionsListCount: 1,
        pResolutionList: &raw const resolution,
        MaxReferenceFramesInDPB: 0,
        ValidationFlags: D3D12_VIDEO_ENCODER_VALIDATION_FLAGS::default(),
        SupportFlags: D3D12_VIDEO_ENCODER_SUPPORT_FLAGS::default(),
        SuggestedProfile: D3D12_VIDEO_ENCODER_PROFILE_DESC {
            DataSize: data_size::<D3D12_VIDEO_ENCODER_AV1_PROFILE>(),
            Anonymous: D3D12_VIDEO_ENCODER_PROFILE_DESC_0 {
                pAV1Profile: &raw mut suggested_profile_av1,
            },
        },
        SuggestedLevel: D3D12_VIDEO_ENCODER_LEVEL_SETTING {
            DataSize: data_size::<D3D12_VIDEO_ENCODER_AV1_LEVEL_TIER_CONSTRAINTS>(),
            Anonymous: D3D12_VIDEO_ENCODER_LEVEL_SETTING_0 {
                pAV1LevelSetting: &raw mut suggested_level_av1,
            },
        },
        pResolutionDependentSupport: &raw mut resolution_limits,
        // No subregion (tile) layout requested — this backend always encodes
        // `SubregionFrameEncoding = FULL_FRAME`, so this new SUPPORT1 input field carries
        // no data, matching `D3D12_VIDEO_ENCODER_SEQUENCE_CONTROL_DESC`'s per-frame
        // counterpart elsewhere in this backend.
        SubregionFrameEncodingData:
            D3D12_VIDEO_ENCODER_PICTURE_CONTROL_SUBREGIONS_LAYOUT_DATA::default(),
        MaxQualityVsSpeed: 0,
    };
    // SAFETY: `support` is sized/typed exactly as `D3D12_FEATURE_DATA_VIDEO_ENCODER_SUPPORT1`
    // expects; every embedded pointer targets a same-scope local valid for the call.
    unsafe {
        video_device
            .CheckFeatureSupport(
                D3D12_FEATURE_VIDEO_ENCODER_SUPPORT1,
                std::ptr::from_mut(&mut support).cast(),
                data_size::<D3D12_FEATURE_DATA_VIDEO_ENCODER_SUPPORT1>(),
            )
            .map_err(|_| EncodeError::Unsupported)?;
    }
    let _ = suggested_profile_av1; // driver-suggested profile — we always request Main, ignored
    if !support
        .SupportFlags
        .contains(D3D12_VIDEO_ENCODER_SUPPORT_FLAG_GENERAL_SUPPORT_OK)
    {
        return Err(EncodeError::Unsupported);
    }
    Ok(suggested_level_av1)
}

pub(super) fn create_encoder(
    video_device: &ID3D12VideoDevice3,
    resolution: D3D12_VIDEO_ENCODER_PICTURE_RESOLUTION_DESC,
    level: D3D12_VIDEO_ENCODER_AV1_LEVEL_TIER_CONSTRAINTS,
) -> Result<(ID3D12VideoEncoder, ID3D12VideoEncoderHeap), EncodeError> {
    let mut profile_av1 = D3D12_VIDEO_ENCODER_AV1_PROFILE_MAIN;
    let mut codec_conf_av1 = default_codec_config_av1();
    let encoder_desc = D3D12_VIDEO_ENCODER_DESC {
        NodeMask: 0,
        Flags: D3D12_VIDEO_ENCODER_FLAG_NONE,
        EncodeCodec: D3D12_VIDEO_ENCODER_CODEC_AV1,
        EncodeProfile: profile_desc_main(&mut profile_av1),
        InputFormat: DXGI_FORMAT_NV12,
        CodecConfiguration: D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION {
            DataSize: data_size::<D3D12_VIDEO_ENCODER_AV1_CODEC_CONFIGURATION>(),
            Anonymous: D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_0 {
                pAV1Config: &raw mut codec_conf_av1,
            },
        },
        MaxMotionEstimationPrecision: D3D12_VIDEO_ENCODER_MOTION_ESTIMATION_PRECISION_MODE_MAXIMUM,
    };
    // SAFETY: `encoder_desc` embeds valid pointers to same-scope locals for the call's duration.
    let encoder: ID3D12VideoEncoder =
        unsafe { video_device.CreateVideoEncoder(&raw const encoder_desc) }
            .map_err(|_| EncodeError::Backend)?;

    let mut level = level;
    let heap_desc = D3D12_VIDEO_ENCODER_HEAP_DESC {
        NodeMask: 0,
        Flags: D3D12_VIDEO_ENCODER_HEAP_FLAG_NONE,
        EncodeCodec: D3D12_VIDEO_ENCODER_CODEC_AV1,
        EncodeProfile: profile_desc_main(&mut profile_av1),
        EncodeLevel: D3D12_VIDEO_ENCODER_LEVEL_SETTING {
            DataSize: data_size::<D3D12_VIDEO_ENCODER_AV1_LEVEL_TIER_CONSTRAINTS>(),
            Anonymous: D3D12_VIDEO_ENCODER_LEVEL_SETTING_0 {
                pAV1LevelSetting: &raw mut level,
            },
        },
        ResolutionsListCount: 1,
        pResolutionList: &raw const resolution,
    };
    // SAFETY: `heap_desc` embeds valid pointers to same-scope locals for the call's duration.
    let encoder_heap: ID3D12VideoEncoderHeap =
        unsafe { video_device.CreateVideoEncoderHeap(&raw const heap_desc) }
            .map_err(|_| EncodeError::Backend)?;

    Ok((encoder, encoder_heap))
}
