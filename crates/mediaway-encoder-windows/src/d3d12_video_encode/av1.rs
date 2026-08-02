//! `open`-time D3D12 object creation for AV1 — mirrors [`super::hevc`]'s HEVC feature
//! queries and `ID3D12VideoEncoder`/`ID3D12VideoEncoderHeap` creation, but for
//! `D3D12_VIDEO_ENCODER_CODEC_AV1` / `D3D12_VIDEO_ENCODER_AV1_PROFILE_MAIN`. Kept as a
//! separate file for the same C-union reasons [`super::hevc`]'s doc comment explains.
//!
//! This backend requests the most conservative AV1 codec configuration available —
//! `D3D12_VIDEO_ENCODER_AV1_FEATURE_FLAG_NONE` (no optional tool enabled) — matching
//! [`super::bitstream_av1`]'s sequence header, which disables every corresponding
//! `enable_*` flag. Unlike [`super::hevc`], this file has not needed a real-hardware
//! codec-configuration sweep (no `NOT_SUPPORTED` driver quirk found for AV1 `FEATURE_FLAG_NONE`
//! on this workspace's RTX 4090 — see this crate's hardware-gated tests).

use mediaway_encoder::EncodeError;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_NV12;
use windows::Win32::Graphics::Dxgi::Common::DXGI_RATIONAL;
use windows::Win32::Media::MediaFoundation::{
    D3D12_FEATURE_DATA_VIDEO_ENCODER_RESOLUTION_SUPPORT_LIMITS,
    D3D12_FEATURE_DATA_VIDEO_ENCODER_RESOURCE_REQUIREMENTS,
    D3D12_FEATURE_DATA_VIDEO_ENCODER_SUPPORT, D3D12_FEATURE_VIDEO_ENCODER_RESOURCE_REQUIREMENTS,
    D3D12_FEATURE_VIDEO_ENCODER_SUPPORT, D3D12_VIDEO_ENCODER_AV1_CODEC_CONFIGURATION,
    D3D12_VIDEO_ENCODER_AV1_FEATURE_FLAG_NONE, D3D12_VIDEO_ENCODER_AV1_LEVEL_TIER_CONSTRAINTS,
    D3D12_VIDEO_ENCODER_AV1_PROFILE, D3D12_VIDEO_ENCODER_AV1_PROFILE_MAIN,
    D3D12_VIDEO_ENCODER_AV1_SEQUENCE_STRUCTURE, D3D12_VIDEO_ENCODER_CODEC_AV1,
    D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION, D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_0,
    D3D12_VIDEO_ENCODER_DESC, D3D12_VIDEO_ENCODER_FLAG_NONE,
    D3D12_VIDEO_ENCODER_FRAME_SUBREGION_LAYOUT_MODE_FULL_FRAME, D3D12_VIDEO_ENCODER_HEAP_DESC,
    D3D12_VIDEO_ENCODER_HEAP_FLAG_NONE, D3D12_VIDEO_ENCODER_INTRA_REFRESH_MODE_NONE,
    D3D12_VIDEO_ENCODER_LEVEL_SETTING, D3D12_VIDEO_ENCODER_LEVEL_SETTING_0,
    D3D12_VIDEO_ENCODER_MOTION_ESTIMATION_PRECISION_MODE_MAXIMUM,
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

/// Most conservative AV1 codec configuration — no optional tool enabled. Matches
/// [`super::bitstream_av1`]'s sequence header, which disables every corresponding
/// `enable_*` flag (superres, CDEF, loop restoration, screen-content tools, warped
/// motion, …).
pub(super) const fn default_codec_config_av1() -> D3D12_VIDEO_ENCODER_AV1_CODEC_CONFIGURATION {
    D3D12_VIDEO_ENCODER_AV1_CODEC_CONFIGURATION {
        FeatureFlags: D3D12_VIDEO_ENCODER_AV1_FEATURE_FLAG_NONE,
        OrderHintBitsMinus1: 0,
    }
}

/// Query `D3D12_FEATURE_VIDEO_ENCODER_SUPPORT` for the exact AV1 codec/GOP/rate-control/
/// resolution combination this session will use (using [`default_codec_config_av1`]'s
/// fixed codec configuration), returning the driver's `SuggestedLevel` (level **and**
/// tier — AV1 levels are tier-qualified like HEVC's, unlike H.264's).
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

    let mut support = D3D12_FEATURE_DATA_VIDEO_ENCODER_SUPPORT {
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
