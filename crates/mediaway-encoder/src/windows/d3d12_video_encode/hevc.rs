//! `open`-time D3D12 object creation for HEVC — mirrors [`super::setup`]'s H.264 feature
//! queries and `ID3D12VideoEncoder`/`ID3D12VideoEncoderHeap` creation, but for
//! `D3D12_VIDEO_ENCODER_CODEC_HEVC` / `D3D12_VIDEO_ENCODER_PROFILE_HEVC_MAIN`. Kept as a
//! separate file rather than folding into `setup.rs`: every D3D12 video-encode struct that
//! carries codec-specific state does so through a C union of `*mut <Codec>Specific`
//! pointers (`D3D12_VIDEO_ENCODER_PROFILE_DESC`, `_CODEC_CONFIGURATION`,
//! `_SEQUENCE_GOP_STRUCTURE`, `_PICTURE_CONTROL_CODEC_DATA`, `_LEVEL_SETTING`) — there is no
//! codec-generic way to build these without threading a codec enum through every builder,
//! so this backend follows the existing H.264 file's straight-line style per codec instead.
//!
//! **Real-hardware finding (RTX 4090, this session):**
//! `D3D12_FEATURE_VIDEO_ENCODER_CODEC_CONFIGURATION_SUPPORT` — the query that would let
//! this backend discover the driver's actual supported coding-unit/transform-unit size
//! range (mirroring how H.264's `SuggestedLevel` is queried, not hardcoded) — reports
//! `IsSupported == false` unconditionally on this driver for HEVC Main, regardless of the
//! `Profile` passed in, even though basic codec support
//! ([`check_codec_support`]), output resolution, and resource requirements all report
//! supported. This looks like the query itself being unimplemented for HEVC on this
//! driver, not a real "HEVC unsupported" answer — confirmed by then feeding candidate
//! coding-unit/transform-unit configurations straight into
//! `D3D12_FEATURE_VIDEO_ENCODER_SUPPORT` (the query this backend's H.264 side already uses
//! to validate its own hardcoded codec config) and observing `GENERAL_SUPPORT_OK` actually
//! flip on for a specific configuration. See [`default_codec_config_hevc`] for the
//! resulting fixed config and how it was found (`ValidationFlags` on failure consistently
//! reported `CODEC_CONFIGURATION_NOT_SUPPORTED`, narrowing the search to exactly this
//! struct).

use crate::EncodeError;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_NV12;
use windows::Win32::Graphics::Dxgi::Common::DXGI_RATIONAL;
use windows::Win32::Media::MediaFoundation::{
    D3D12_FEATURE_DATA_VIDEO_ENCODER_RESOLUTION_SUPPORT_LIMITS,
    D3D12_FEATURE_DATA_VIDEO_ENCODER_RESOURCE_REQUIREMENTS,
    D3D12_FEATURE_DATA_VIDEO_ENCODER_SUPPORT, D3D12_FEATURE_VIDEO_ENCODER_RESOURCE_REQUIREMENTS,
    D3D12_FEATURE_VIDEO_ENCODER_SUPPORT, D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION,
    D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_0, D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_HEVC,
    D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_HEVC_CUSIZE_8x8,
    D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_HEVC_CUSIZE_32x32,
    D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_HEVC_FLAG_USE_ASYMETRIC_MOTION_PARTITION,
    D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_HEVC_TUSIZE_4x4,
    D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_HEVC_TUSIZE_32x32, D3D12_VIDEO_ENCODER_CODEC_HEVC,
    D3D12_VIDEO_ENCODER_DESC, D3D12_VIDEO_ENCODER_FLAG_NONE,
    D3D12_VIDEO_ENCODER_FRAME_SUBREGION_LAYOUT_MODE_FULL_FRAME, D3D12_VIDEO_ENCODER_HEAP_DESC,
    D3D12_VIDEO_ENCODER_HEAP_FLAG_NONE, D3D12_VIDEO_ENCODER_INTRA_REFRESH_MODE_NONE,
    D3D12_VIDEO_ENCODER_LEVEL_SETTING, D3D12_VIDEO_ENCODER_LEVEL_SETTING_0,
    D3D12_VIDEO_ENCODER_LEVEL_TIER_CONSTRAINTS_HEVC, D3D12_VIDEO_ENCODER_LEVELS_HEVC,
    D3D12_VIDEO_ENCODER_LEVELS_HEVC_61,
    D3D12_VIDEO_ENCODER_MOTION_ESTIMATION_PRECISION_MODE_MAXIMUM,
    D3D12_VIDEO_ENCODER_PICTURE_RESOLUTION_DESC, D3D12_VIDEO_ENCODER_PROFILE_DESC,
    D3D12_VIDEO_ENCODER_PROFILE_DESC_0, D3D12_VIDEO_ENCODER_PROFILE_HEVC,
    D3D12_VIDEO_ENCODER_PROFILE_HEVC_MAIN, D3D12_VIDEO_ENCODER_RATE_CONTROL,
    D3D12_VIDEO_ENCODER_RATE_CONTROL_CONFIGURATION_PARAMS,
    D3D12_VIDEO_ENCODER_RATE_CONTROL_CONFIGURATION_PARAMS_0, D3D12_VIDEO_ENCODER_RATE_CONTROL_CQP,
    D3D12_VIDEO_ENCODER_RATE_CONTROL_FLAG_NONE, D3D12_VIDEO_ENCODER_RATE_CONTROL_MODE_CQP,
    D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE, D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_0,
    D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_HEVC,
    D3D12_VIDEO_ENCODER_SUPPORT_FLAG_GENERAL_SUPPORT_OK, D3D12_VIDEO_ENCODER_SUPPORT_FLAGS,
    D3D12_VIDEO_ENCODER_TIER_HEVC_HIGH, D3D12_VIDEO_ENCODER_VALIDATION_FLAGS, ID3D12VideoDevice3,
    ID3D12VideoEncoder, ID3D12VideoEncoderHeap,
};
use windows::core::BOOL;

use super::setup::{
    check_codec_support as check_codec_support_generic,
    check_output_resolution as check_output_resolution_generic,
};
use super::util::data_size;

/// `pic_width`/`height_in_luma_samples` must be a multiple of this (`MinCbSizeY`, the SPS's
/// minimum coding-block size — see [`default_codec_config_hevc`]) — a bitstream-conformance
/// requirement, since this backend never signals a `conformance_window` crop.
pub(super) const MIN_CB_SIZE_PIXELS: u32 = 8;

pub(super) fn check_codec_support(video_device: &ID3D12VideoDevice3) -> Result<(), EncodeError> {
    check_codec_support_generic(video_device, D3D12_VIDEO_ENCODER_CODEC_HEVC)
}

pub(super) fn check_output_resolution(
    video_device: &ID3D12VideoDevice3,
    resolution: D3D12_VIDEO_ENCODER_PICTURE_RESOLUTION_DESC,
) -> Result<(), EncodeError> {
    check_output_resolution_generic(video_device, D3D12_VIDEO_ENCODER_CODEC_HEVC, resolution)
}

fn profile_desc_main(
    profile_hevc: &mut D3D12_VIDEO_ENCODER_PROFILE_HEVC,
) -> D3D12_VIDEO_ENCODER_PROFILE_DESC {
    D3D12_VIDEO_ENCODER_PROFILE_DESC {
        DataSize: data_size::<D3D12_VIDEO_ENCODER_PROFILE_HEVC>(),
        Anonymous: D3D12_VIDEO_ENCODER_PROFILE_DESC_0 {
            pHEVCProfile: profile_hevc,
        },
    }
}

pub(super) fn check_resource_requirements(
    video_device: &ID3D12VideoDevice3,
    resolution: D3D12_VIDEO_ENCODER_PICTURE_RESOLUTION_DESC,
) -> Result<D3D12_FEATURE_DATA_VIDEO_ENCODER_RESOURCE_REQUIREMENTS, EncodeError> {
    let mut profile_hevc = D3D12_VIDEO_ENCODER_PROFILE_HEVC_MAIN;
    let mut req = D3D12_FEATURE_DATA_VIDEO_ENCODER_RESOURCE_REQUIREMENTS {
        NodeIndex: 0,
        Codec: D3D12_VIDEO_ENCODER_CODEC_HEVC,
        Profile: profile_desc_main(&mut profile_hevc),
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

/// Fixed HEVC codec configuration for this backend — **not** driver-queried (see this
/// file's module doc for why): coding-unit size `8x8..32x32`, transform-unit size
/// `4x4..32x32`, transform-hierarchy depth `3` for both inter and intra
/// (`log2(32) - log2(4) == 3`, the legal maximum for this CU/TU range). No SAO/transform-
/// skip/long-term references. Found by sweeping `D3D12_FEATURE_VIDEO_ENCODER_SUPPORT`'s
/// `GENERAL_SUPPORT_OK` flag against candidate configurations on real hardware —
/// `MaxLumaCodingUnitSize` specifically must be `32x32` (not `64x64`, despite that being
/// the larger/"more capable" choice) or the driver reports
/// `VALIDATION_FLAG_CODEC_CONFIGURATION_NOT_SUPPORTED`. `USE_ASYMETRIC_MOTION_PARTITION`
/// is **required**, not optional: `GENERAL_SUPPORT_OK` from the query above stays true
/// without it, but the real `ID3D12VideoDevice3::CreateVideoEncoder` call then fails with
/// the D3D12 debug layer reporting, verbatim, "Asymetric motion partition is required to
/// be set" — a case where `CheckFeatureSupport`'s advisory result under-reports what
/// `CreateVideoEncoder` actually enforces, only found by running the real call and reading
/// `ID3D12InfoQueue`.
pub(super) const fn default_codec_config_hevc() -> D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_HEVC {
    D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_HEVC {
        ConfigurationFlags:
            D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_HEVC_FLAG_USE_ASYMETRIC_MOTION_PARTITION,
        MinLumaCodingUnitSize: D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_HEVC_CUSIZE_8x8,
        MaxLumaCodingUnitSize: D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_HEVC_CUSIZE_32x32,
        MinLumaTransformUnitSize: D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_HEVC_TUSIZE_4x4,
        MaxLumaTransformUnitSize: D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_HEVC_TUSIZE_32x32,
        max_transform_hierarchy_depth_inter: 3,
        max_transform_hierarchy_depth_intra: 3,
    }
}

/// Query `D3D12_FEATURE_VIDEO_ENCODER_SUPPORT` for the exact HEVC codec/GOP/rate-control/
/// resolution combination this session will use (using [`default_codec_config_hevc`]'s
/// fixed codec configuration), returning the driver's `SuggestedLevel` (level **and**
/// tier — HEVC levels are tier-qualified, unlike H.264's). This is the query that actually
/// gates whether `open` succeeds for HEVC — see this file's module doc.
pub(super) fn check_encoder_support(
    video_device: &ID3D12VideoDevice3,
    resolution: D3D12_VIDEO_ENCODER_PICTURE_RESOLUTION_DESC,
    mut gop: D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_HEVC,
    rc_cqp: D3D12_VIDEO_ENCODER_RATE_CONTROL_CQP,
    frame_rate: (u32, u32),
    max_reference_frames_in_dpb: u32,
) -> Result<D3D12_VIDEO_ENCODER_LEVEL_TIER_CONSTRAINTS_HEVC, EncodeError> {
    let mut codec_conf_hevc = default_codec_config_hevc();
    let mut resolution_limits =
        D3D12_FEATURE_DATA_VIDEO_ENCODER_RESOLUTION_SUPPORT_LIMITS::default();
    let mut suggested_profile_hevc = D3D12_VIDEO_ENCODER_PROFILE_HEVC_MAIN;
    let mut suggested_level_hevc = D3D12_VIDEO_ENCODER_LEVEL_TIER_CONSTRAINTS_HEVC {
        Level: D3D12_VIDEO_ENCODER_LEVELS_HEVC_61,
        Tier: D3D12_VIDEO_ENCODER_TIER_HEVC_HIGH,
    };

    let mut support = D3D12_FEATURE_DATA_VIDEO_ENCODER_SUPPORT {
        NodeIndex: 0,
        Codec: D3D12_VIDEO_ENCODER_CODEC_HEVC,
        InputFormat: DXGI_FORMAT_NV12,
        CodecConfiguration: D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION {
            DataSize: data_size::<D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_HEVC>(),
            Anonymous: D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_0 {
                pHEVCConfig: &raw mut codec_conf_hevc,
            },
        },
        CodecGopSequence: D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE {
            DataSize: data_size::<D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_HEVC>(),
            Anonymous: D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_0 {
                pHEVCGroupOfPictures: &raw mut gop,
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
        MaxReferenceFramesInDPB: max_reference_frames_in_dpb,
        ValidationFlags: D3D12_VIDEO_ENCODER_VALIDATION_FLAGS::default(),
        SupportFlags: D3D12_VIDEO_ENCODER_SUPPORT_FLAGS::default(),
        SuggestedProfile: D3D12_VIDEO_ENCODER_PROFILE_DESC {
            DataSize: data_size::<D3D12_VIDEO_ENCODER_PROFILE_HEVC>(),
            Anonymous: D3D12_VIDEO_ENCODER_PROFILE_DESC_0 {
                pHEVCProfile: &raw mut suggested_profile_hevc,
            },
        },
        SuggestedLevel: D3D12_VIDEO_ENCODER_LEVEL_SETTING {
            DataSize: data_size::<D3D12_VIDEO_ENCODER_LEVEL_TIER_CONSTRAINTS_HEVC>(),
            Anonymous: D3D12_VIDEO_ENCODER_LEVEL_SETTING_0 {
                pHEVCLevelSetting: &raw mut suggested_level_hevc,
            },
        },
        pResolutionDependentSupport: &raw mut resolution_limits,
    };
    // SAFETY: `support` is sized/typed exactly as `D3D12_FEATURE_DATA_VIDEO_ENCODER_SUPPORT`
    // expects; every embedded pointer targets a same-scope local valid for the call.
    unsafe {
        video_device
            .CheckFeatureSupport(
                D3D12_FEATURE_VIDEO_ENCODER_SUPPORT,
                std::ptr::from_mut(&mut support).cast(),
                data_size::<D3D12_FEATURE_DATA_VIDEO_ENCODER_SUPPORT>(),
            )
            .map_err(|_| EncodeError::Unsupported)?;
    }
    let _ = suggested_profile_hevc; // driver-suggested profile — we always request Main, ignored
    if !support
        .SupportFlags
        .contains(D3D12_VIDEO_ENCODER_SUPPORT_FLAG_GENERAL_SUPPORT_OK)
    {
        return Err(EncodeError::Unsupported);
    }
    Ok(suggested_level_hevc)
}

pub(super) fn create_encoder(
    video_device: &ID3D12VideoDevice3,
    resolution: D3D12_VIDEO_ENCODER_PICTURE_RESOLUTION_DESC,
    level: D3D12_VIDEO_ENCODER_LEVEL_TIER_CONSTRAINTS_HEVC,
) -> Result<(ID3D12VideoEncoder, ID3D12VideoEncoderHeap), EncodeError> {
    let mut profile_hevc = D3D12_VIDEO_ENCODER_PROFILE_HEVC_MAIN;
    let mut codec_conf_hevc = default_codec_config_hevc();
    let encoder_desc = D3D12_VIDEO_ENCODER_DESC {
        NodeMask: 0,
        Flags: D3D12_VIDEO_ENCODER_FLAG_NONE,
        EncodeCodec: D3D12_VIDEO_ENCODER_CODEC_HEVC,
        EncodeProfile: profile_desc_main(&mut profile_hevc),
        InputFormat: DXGI_FORMAT_NV12,
        CodecConfiguration: D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION {
            DataSize: data_size::<D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_HEVC>(),
            Anonymous: D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_0 {
                pHEVCConfig: &raw mut codec_conf_hevc,
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
        EncodeCodec: D3D12_VIDEO_ENCODER_CODEC_HEVC,
        EncodeProfile: profile_desc_main(&mut profile_hevc),
        EncodeLevel: D3D12_VIDEO_ENCODER_LEVEL_SETTING {
            DataSize: data_size::<D3D12_VIDEO_ENCODER_LEVEL_TIER_CONSTRAINTS_HEVC>(),
            Anonymous: D3D12_VIDEO_ENCODER_LEVEL_SETTING_0 {
                pHEVCLevelSetting: &raw mut level,
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

/// HEVC `general_level_idc` (Rec. ITU-T H.265 §A.3, `Level * 30`) for a
/// `D3D12_VIDEO_ENCODER_LEVELS_HEVC` value.
pub(super) const fn level_hevc_to_general_level_idc(level: D3D12_VIDEO_ENCODER_LEVELS_HEVC) -> u8 {
    match level.0 {
        0 => 30,   // Level 1
        1 => 60,   // Level 2
        2 => 63,   // Level 2.1
        3 => 90,   // Level 3
        4 => 93,   // Level 3.1
        5 => 120,  // Level 4
        6 => 123,  // Level 4.1
        7 => 150,  // Level 5
        8 => 153,  // Level 5.1
        9 => 156,  // Level 5.2
        10 => 180, // Level 6
        11 => 183, // Level 6.1
        _ => 186,  // Level 6.2 (and any future/unknown value — highest known level)
    }
}
