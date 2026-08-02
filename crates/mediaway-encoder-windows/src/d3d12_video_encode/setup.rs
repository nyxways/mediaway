//! `open`-time D3D12 object creation: device/queue/allocator/list objects, feature-support
//! queries, and the `ID3D12VideoEncoder`/`ID3D12VideoEncoderHeap` pair.

use mediaway_common::NativeHandle;
use mediaway_encoder::EncodeError;
use windows::Win32::Graphics::Direct3D12::{
    D3D12_COMMAND_LIST_FLAG_NONE, D3D12_COMMAND_LIST_TYPE, D3D12_COMMAND_QUEUE_DESC,
    D3D12_COMMAND_QUEUE_FLAG_NONE, D3D12_CPU_PAGE_PROPERTY_UNKNOWN, D3D12_HEAP_FLAG_NONE,
    D3D12_HEAP_PROPERTIES, D3D12_HEAP_TYPE, D3D12_HEAP_TYPE_DEFAULT, D3D12_MEMORY_POOL_UNKNOWN,
    D3D12_RESOURCE_DESC, D3D12_RESOURCE_DIMENSION_BUFFER, D3D12_RESOURCE_DIMENSION_TEXTURE2D,
    D3D12_RESOURCE_FLAG_NONE, D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATES,
    D3D12_TEXTURE_LAYOUT_ROW_MAJOR, D3D12_TEXTURE_LAYOUT_UNKNOWN, ID3D12CommandAllocator,
    ID3D12CommandQueue, ID3D12Device, ID3D12Device4, ID3D12Resource,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_RATIONAL;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_NV12, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC,
};
use windows::Win32::Media::MediaFoundation::{
    D3D12_FEATURE_DATA_VIDEO_ENCODER_CODEC, D3D12_FEATURE_DATA_VIDEO_ENCODER_OUTPUT_RESOLUTION,
    D3D12_FEATURE_DATA_VIDEO_ENCODER_RESOLUTION_SUPPORT_LIMITS,
    D3D12_FEATURE_DATA_VIDEO_ENCODER_RESOURCE_REQUIREMENTS,
    D3D12_FEATURE_DATA_VIDEO_ENCODER_SUPPORT, D3D12_FEATURE_VIDEO_ENCODER_CODEC,
    D3D12_FEATURE_VIDEO_ENCODER_OUTPUT_RESOLUTION,
    D3D12_FEATURE_VIDEO_ENCODER_RESOURCE_REQUIREMENTS, D3D12_FEATURE_VIDEO_ENCODER_SUPPORT,
    D3D12_VIDEO_ENCODER_CODEC, D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION,
    D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_0, D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_H264,
    D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_H264_DIRECT_MODES_DISABLED,
    D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_H264_FLAG_NONE,
    D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_H264_SLICES_DEBLOCKING_MODE_0_ALL_LUMA_CHROMA_SLICE_BLOCK_EDGES_ALWAYS_FILTERED,
    D3D12_VIDEO_ENCODER_CODEC_H264, D3D12_VIDEO_ENCODER_DESC, D3D12_VIDEO_ENCODER_FLAG_NONE,
    D3D12_VIDEO_ENCODER_FRAME_SUBREGION_LAYOUT_MODE_FULL_FRAME, D3D12_VIDEO_ENCODER_HEAP_DESC,
    D3D12_VIDEO_ENCODER_HEAP_FLAG_NONE, D3D12_VIDEO_ENCODER_INTRA_REFRESH_MODE_NONE,
    D3D12_VIDEO_ENCODER_LEVEL_SETTING, D3D12_VIDEO_ENCODER_LEVEL_SETTING_0,
    D3D12_VIDEO_ENCODER_LEVELS_H264, D3D12_VIDEO_ENCODER_LEVELS_H264_51,
    D3D12_VIDEO_ENCODER_MOTION_ESTIMATION_PRECISION_MODE_MAXIMUM,
    D3D12_VIDEO_ENCODER_PICTURE_RESOLUTION_DESC, D3D12_VIDEO_ENCODER_PROFILE_DESC,
    D3D12_VIDEO_ENCODER_PROFILE_DESC_0, D3D12_VIDEO_ENCODER_PROFILE_H264,
    D3D12_VIDEO_ENCODER_PROFILE_H264_MAIN, D3D12_VIDEO_ENCODER_RATE_CONTROL,
    D3D12_VIDEO_ENCODER_RATE_CONTROL_CONFIGURATION_PARAMS,
    D3D12_VIDEO_ENCODER_RATE_CONTROL_CONFIGURATION_PARAMS_0, D3D12_VIDEO_ENCODER_RATE_CONTROL_CQP,
    D3D12_VIDEO_ENCODER_RATE_CONTROL_FLAG_NONE, D3D12_VIDEO_ENCODER_RATE_CONTROL_MODE_CQP,
    D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE, D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_0,
    D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_H264,
    D3D12_VIDEO_ENCODER_SUPPORT_FLAG_GENERAL_SUPPORT_OK, D3D12_VIDEO_ENCODER_SUPPORT_FLAGS,
    D3D12_VIDEO_ENCODER_VALIDATION_FLAGS, ID3D12VideoDevice3, ID3D12VideoEncoder,
    ID3D12VideoEncoderHeap,
};
use windows::core::{BOOL, Interface};

use super::util::data_size;

/// Borrow `device_handle` as an owned `ID3D12Device` (COM `AddRef`d for this session).
pub(super) fn device_from_handle(device_handle: NativeHandle) -> Result<ID3D12Device, EncodeError> {
    let raw = device_handle.get() as *mut core::ffi::c_void;
    // SAFETY: caller guarantees a live `ID3D12Device*` for the session (config contract).
    let borrowed =
        unsafe { ID3D12Device::from_raw_borrowed(&raw) }.ok_or(EncodeError::InvalidInput)?;
    // clone: COM AddRef so we own the device for the session's lifetime
    Ok(borrowed.clone())
}

/// Check `D3D12_FEATURE_VIDEO_ENCODER_CODEC` for `codec`. Codec-generic (the query struct
/// only varies by the `Codec` field) — shared by [`super::hevc::check_codec_support`] as
/// well as this file's H.264 call site.
pub(super) fn check_codec_support(
    video_device: &ID3D12VideoDevice3,
    codec: D3D12_VIDEO_ENCODER_CODEC,
) -> Result<(), EncodeError> {
    let mut support = D3D12_FEATURE_DATA_VIDEO_ENCODER_CODEC {
        NodeIndex: 0,
        Codec: codec,
        IsSupported: BOOL::default(),
    };
    // SAFETY: `support` is sized/typed exactly as `D3D12_FEATURE_VIDEO_ENCODER_CODEC` expects.
    unsafe {
        video_device
            .CheckFeatureSupport(
                D3D12_FEATURE_VIDEO_ENCODER_CODEC,
                std::ptr::from_mut(&mut support).cast(),
                data_size::<D3D12_FEATURE_DATA_VIDEO_ENCODER_CODEC>(),
            )
            .map_err(|_| EncodeError::Unsupported)?;
    }
    if !support.IsSupported.as_bool() {
        return Err(EncodeError::Unsupported);
    }
    Ok(())
}

fn profile_desc_main(
    profile_h264: &mut D3D12_VIDEO_ENCODER_PROFILE_H264,
) -> D3D12_VIDEO_ENCODER_PROFILE_DESC {
    D3D12_VIDEO_ENCODER_PROFILE_DESC {
        DataSize: data_size::<D3D12_VIDEO_ENCODER_PROFILE_H264>(),
        Anonymous: D3D12_VIDEO_ENCODER_PROFILE_DESC_0 {
            pH264Profile: profile_h264,
        },
    }
}

/// Validate `resolution` against `D3D12_FEATURE_VIDEO_ENCODER_OUTPUT_RESOLUTION`'s real
/// per-driver min/max/multiple-of constraints.
///
/// Real hardware enforces a nonzero **minimum** encode resolution (observed: 160x64 on an
/// NVIDIA RTX 4090) — below it, `CreateVideoEncoderHeap` fails with a raw `E_INVALIDARG`
/// that gives the caller no indication *why*. Checking this upfront turns that into a
/// clear [`EncodeError::Unsupported`] instead.
/// Codec-generic sibling of [`check_codec_support`] — see that function's doc comment.
pub(super) fn check_output_resolution(
    video_device: &ID3D12VideoDevice3,
    codec: D3D12_VIDEO_ENCODER_CODEC,
    resolution: D3D12_VIDEO_ENCODER_PICTURE_RESOLUTION_DESC,
) -> Result<(), EncodeError> {
    let mut res = D3D12_FEATURE_DATA_VIDEO_ENCODER_OUTPUT_RESOLUTION {
        NodeIndex: 0,
        Codec: codec,
        ResolutionRatiosCount: 0,
        IsSupported: BOOL::default(),
        MinResolutionSupported: D3D12_VIDEO_ENCODER_PICTURE_RESOLUTION_DESC::default(),
        MaxResolutionSupported: D3D12_VIDEO_ENCODER_PICTURE_RESOLUTION_DESC::default(),
        ResolutionWidthMultipleRequirement: 0,
        ResolutionHeightMultipleRequirement: 0,
        pResolutionRatios: std::ptr::null_mut(),
    };
    // SAFETY: `res` is sized/typed exactly as `D3D12_FEATURE_VIDEO_ENCODER_OUTPUT_RESOLUTION` expects.
    unsafe {
        video_device
            .CheckFeatureSupport(
                D3D12_FEATURE_VIDEO_ENCODER_OUTPUT_RESOLUTION,
                std::ptr::from_mut(&mut res).cast(),
                data_size::<D3D12_FEATURE_DATA_VIDEO_ENCODER_OUTPUT_RESOLUTION>(),
            )
            .map_err(|_| EncodeError::Unsupported)?;
    }
    if !res.IsSupported.as_bool() {
        return Err(EncodeError::Unsupported);
    }
    let in_range = resolution.Width >= res.MinResolutionSupported.Width
        && resolution.Width <= res.MaxResolutionSupported.Width
        && resolution.Height >= res.MinResolutionSupported.Height
        && resolution.Height <= res.MaxResolutionSupported.Height;
    let width_mul_ok = res.ResolutionWidthMultipleRequirement == 0
        || resolution.Width % res.ResolutionWidthMultipleRequirement == 0;
    let height_mul_ok = res.ResolutionHeightMultipleRequirement == 0
        || resolution.Height % res.ResolutionHeightMultipleRequirement == 0;
    if !in_range || !width_mul_ok || !height_mul_ok {
        return Err(EncodeError::Unsupported);
    }
    Ok(())
}

pub(super) fn check_resource_requirements(
    video_device: &ID3D12VideoDevice3,
    resolution: D3D12_VIDEO_ENCODER_PICTURE_RESOLUTION_DESC,
) -> Result<D3D12_FEATURE_DATA_VIDEO_ENCODER_RESOURCE_REQUIREMENTS, EncodeError> {
    let mut profile_h264 = D3D12_VIDEO_ENCODER_PROFILE_H264_MAIN;
    let mut req = D3D12_FEATURE_DATA_VIDEO_ENCODER_RESOURCE_REQUIREMENTS {
        NodeIndex: 0,
        Codec: D3D12_VIDEO_ENCODER_CODEC_H264,
        Profile: profile_desc_main(&mut profile_h264),
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

/// Fixed H.264 codec configuration for this backend: CAVLC (no CABAC), standard
/// deblocking, no direct-mode B-frame prediction (no B-frames are ever requested).
pub(super) const fn default_codec_config_h264() -> D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_H264 {
    D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_H264 {
        ConfigurationFlags: D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_H264_FLAG_NONE,
        DirectModeConfig: D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_H264_DIRECT_MODES_DISABLED,
        DisableDeblockingFilterConfig:
            D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_H264_SLICES_DEBLOCKING_MODE_0_ALL_LUMA_CHROMA_SLICE_BLOCK_EDGES_ALWAYS_FILTERED,
    }
}

/// Query `D3D12_FEATURE_VIDEO_ENCODER_SUPPORT` for the exact codec/GOP/rate-control/
/// resolution combination this session will use, returning the driver's `SuggestedLevel`
/// (the level actually valid for this configuration on this hardware — a hardcoded guess
/// reliably fails `CreateVideoEncoderHeap` with `E_INVALIDARG` on real drivers).
pub(super) fn check_encoder_support(
    video_device: &ID3D12VideoDevice3,
    resolution: D3D12_VIDEO_ENCODER_PICTURE_RESOLUTION_DESC,
    mut gop: D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_H264,
    rc_cqp: D3D12_VIDEO_ENCODER_RATE_CONTROL_CQP,
    frame_rate: (u32, u32),
) -> Result<D3D12_VIDEO_ENCODER_LEVELS_H264, EncodeError> {
    let mut codec_conf_h264 = default_codec_config_h264();
    let mut resolution_limits =
        D3D12_FEATURE_DATA_VIDEO_ENCODER_RESOLUTION_SUPPORT_LIMITS::default();
    let mut suggested_profile_h264 = D3D12_VIDEO_ENCODER_PROFILE_H264_MAIN;
    let mut suggested_level_h264 = D3D12_VIDEO_ENCODER_LEVELS_H264_51;

    let mut support = D3D12_FEATURE_DATA_VIDEO_ENCODER_SUPPORT {
        NodeIndex: 0,
        Codec: D3D12_VIDEO_ENCODER_CODEC_H264,
        InputFormat: DXGI_FORMAT_NV12,
        CodecConfiguration: D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION {
            DataSize: data_size::<D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_H264>(),
            Anonymous: D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_0 {
                pH264Config: &raw mut codec_conf_h264,
            },
        },
        CodecGopSequence: D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE {
            DataSize: data_size::<D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_H264>(),
            Anonymous: D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_0 {
                pH264GroupOfPictures: &raw mut gop,
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
            DataSize: data_size::<D3D12_VIDEO_ENCODER_PROFILE_H264>(),
            Anonymous: D3D12_VIDEO_ENCODER_PROFILE_DESC_0 {
                pH264Profile: &raw mut suggested_profile_h264,
            },
        },
        SuggestedLevel: D3D12_VIDEO_ENCODER_LEVEL_SETTING {
            DataSize: data_size::<D3D12_VIDEO_ENCODER_LEVELS_H264>(),
            Anonymous: D3D12_VIDEO_ENCODER_LEVEL_SETTING_0 {
                pH264LevelSetting: &raw mut suggested_level_h264,
            },
        },
        pResolutionDependentSupport: &raw mut resolution_limits,
    };
    // SAFETY: `support` is sized/typed exactly as `D3D12_FEATURE_VIDEO_ENCODER_SUPPORT`
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
    let _ = suggested_profile_h264; // driver-suggested profile — we always request Main, ignored
    if !support
        .SupportFlags
        .contains(D3D12_VIDEO_ENCODER_SUPPORT_FLAG_GENERAL_SUPPORT_OK)
    {
        return Err(EncodeError::Unsupported);
    }
    Ok(suggested_level_h264)
}

pub(super) fn create_encoder(
    video_device: &ID3D12VideoDevice3,
    resolution: D3D12_VIDEO_ENCODER_PICTURE_RESOLUTION_DESC,
    level: D3D12_VIDEO_ENCODER_LEVELS_H264,
) -> Result<(ID3D12VideoEncoder, ID3D12VideoEncoderHeap), EncodeError> {
    let mut profile_h264 = D3D12_VIDEO_ENCODER_PROFILE_H264_MAIN;
    let mut codec_conf_h264 = default_codec_config_h264();
    let encoder_desc = D3D12_VIDEO_ENCODER_DESC {
        NodeMask: 0,
        Flags: D3D12_VIDEO_ENCODER_FLAG_NONE,
        EncodeCodec: D3D12_VIDEO_ENCODER_CODEC_H264,
        EncodeProfile: profile_desc_main(&mut profile_h264),
        InputFormat: DXGI_FORMAT_NV12,
        CodecConfiguration: D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION {
            DataSize: data_size::<D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_H264>(),
            Anonymous: D3D12_VIDEO_ENCODER_CODEC_CONFIGURATION_0 {
                pH264Config: &raw mut codec_conf_h264,
            },
        },
        MaxMotionEstimationPrecision: D3D12_VIDEO_ENCODER_MOTION_ESTIMATION_PRECISION_MODE_MAXIMUM,
    };
    // SAFETY: `encoder_desc` embeds valid pointers to same-scope locals for the call's duration.
    let encoder: ID3D12VideoEncoder =
        unsafe { video_device.CreateVideoEncoder(&raw const encoder_desc) }
            .map_err(|_| EncodeError::Backend)?;

    let mut level_h264 = level;
    let heap_desc = D3D12_VIDEO_ENCODER_HEAP_DESC {
        NodeMask: 0,
        Flags: D3D12_VIDEO_ENCODER_HEAP_FLAG_NONE,
        EncodeCodec: D3D12_VIDEO_ENCODER_CODEC_H264,
        EncodeProfile: profile_desc_main(&mut profile_h264),
        EncodeLevel: D3D12_VIDEO_ENCODER_LEVEL_SETTING {
            DataSize: data_size::<D3D12_VIDEO_ENCODER_LEVELS_H264>(),
            Anonymous: D3D12_VIDEO_ENCODER_LEVEL_SETTING_0 {
                pH264LevelSetting: &raw mut level_h264,
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

/// H.264 `level_idc` (as written into the SPS, `level * 10`) for a
/// `D3D12_VIDEO_ENCODER_LEVELS_H264` value. Level 1b (`level_idc == 9`, needs
/// `constraint_set3_flag`) is rounded up to Level 1.1 (`11`) — this backend's minimal SPS
/// never sets that constraint flag, and declaring a slightly higher level than strictly
/// necessary is always spec-safe (never a decode-compatibility issue).
pub(super) const fn level_h264_to_idc(level: D3D12_VIDEO_ENCODER_LEVELS_H264) -> u8 {
    match level.0 {
        0 => 10,     // 1
        1 | 2 => 11, // 1b (rounded up) / 1.1
        3 => 12,     // 1.2
        4 => 13,     // 1.3
        5 => 20,     // 2
        6 => 21,     // 2.1
        7 => 22,     // 2.2
        8 => 30,     // 3
        9 => 31,     // 3.1
        10 => 32,    // 3.2
        11 => 40,    // 4
        12 => 41,    // 4.1
        13 => 42,    // 4.2
        14 => 50,    // 5
        15 => 51,    // 5.1
        16 => 52,    // 5.2
        17 => 60,    // 6
        18 => 61,    // 6.1
        _ => 62,     // 6.2 (and any future/unknown value — highest known level)
    }
}

pub(super) fn create_command_objects<T: Interface>(
    device: &ID3D12Device,
    device4: &ID3D12Device4,
    list_type: D3D12_COMMAND_LIST_TYPE,
) -> Result<(ID3D12CommandQueue, ID3D12CommandAllocator, T), EncodeError> {
    let queue_desc = D3D12_COMMAND_QUEUE_DESC {
        Type: list_type,
        Priority: 0,
        Flags: D3D12_COMMAND_QUEUE_FLAG_NONE,
        NodeMask: 0,
    };
    // SAFETY: plain POD desc, no borrowed pointers.
    let queue: ID3D12CommandQueue = unsafe { device.CreateCommandQueue(&raw const queue_desc) }
        .map_err(|_| EncodeError::Backend)?;
    // SAFETY: `list_type` is a valid `D3D12_COMMAND_LIST_TYPE`.
    let allocator: ID3D12CommandAllocator =
        unsafe { device.CreateCommandAllocator(list_type) }.map_err(|_| EncodeError::Backend)?;
    // SAFETY: `device4` was obtained via `cast` from the same live device.
    let list: T = unsafe { device4.CreateCommandList1(0, list_type, D3D12_COMMAND_LIST_FLAG_NONE) }
        .map_err(|_| EncodeError::Backend)?;
    Ok((queue, allocator, list))
}

pub(super) fn create_nv12_texture(
    device: &ID3D12Device,
    width: u32,
    height: u32,
) -> Result<ID3D12Resource, EncodeError> {
    let heap_props = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_DEFAULT,
        CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
        MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
        CreationNodeMask: 1,
        VisibleNodeMask: 1,
    };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Alignment: 0,
        Width: u64::from(width),
        Height: height,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: DXGI_FORMAT_NV12,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
        Flags: D3D12_RESOURCE_FLAG_NONE,
    };
    let mut resource: Option<ID3D12Resource> = None;
    // SAFETY: `CreateCommittedResource` with a DEFAULT-heap NV12 texture; caller-owned.
    unsafe {
        device
            .CreateCommittedResource(
                &raw const heap_props,
                D3D12_HEAP_FLAG_NONE,
                &raw const desc,
                D3D12_RESOURCE_STATE_COMMON,
                None,
                &raw mut resource,
            )
            .map_err(|_| EncodeError::Backend)?;
    }
    resource.ok_or(EncodeError::Backend)
}

/// Create a linear buffer on the `D3D12_HEAP_TYPE_CUSTOM` equivalent of `heap_type`
/// (via `GetCustomHeapProperties`), rather than passing `heap_type` (e.g.
/// `D3D12_HEAP_TYPE_READBACK`) straight through.
///
/// This matters specifically for buffers this backend later transitions to
/// `D3D12_RESOURCE_STATE_VIDEO_ENCODE_WRITE`/`_READ` (the compressed-bitstream and
/// resolved-metadata outputs): the D3D12 debug layer enforces "resources on
/// `D3D12_HEAP_TYPE_READBACK` heaps support only `COMMON`/`COPY_DEST`/`RESOLVE_DEST`" —
/// a restriction keyed on the **abstract** heap-type enum value, not the underlying
/// memory's actual CPU-visibility/pool characteristics. Resolving to the concrete
/// `D3D12_HEAP_TYPE_CUSTOM` properties upfront (same physical memory, same CPU
/// `Map`/`Unmap` behavior) sidesteps that abstract-type check entirely — this is the
/// same trick `FFmpeg`'s shipped `d3d12va_encode.c` uses for its output/metadata buffers.
pub(super) fn create_linear_buffer(
    device: &ID3D12Device,
    heap_type: D3D12_HEAP_TYPE,
    size: u64,
    initial_state: D3D12_RESOURCE_STATES,
) -> Result<ID3D12Resource, EncodeError> {
    // SAFETY: plain getter, no borrowed pointers.
    let heap_props = unsafe { device.GetCustomHeapProperties(0, heap_type) };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Alignment: 0,
        Width: size.max(1),
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: DXGI_FORMAT_UNKNOWN,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        Flags: D3D12_RESOURCE_FLAG_NONE,
    };
    let mut resource: Option<ID3D12Resource> = None;
    // SAFETY: `CreateCommittedResource` with a linear buffer on `heap_type`; caller-owned.
    unsafe {
        device
            .CreateCommittedResource(
                &raw const heap_props,
                D3D12_HEAP_FLAG_NONE,
                &raw const desc,
                initial_state,
                None,
                &raw mut resource,
            )
            .map_err(|_| EncodeError::Backend)?;
    }
    resource.ok_or(EncodeError::Backend)
}
