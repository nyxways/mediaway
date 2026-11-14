//! Hand-written subset of the NVENC API (`nvEncodeAPI.h`, SDK **13.0**) for the live session.
//!
//! Why not the `nvenc` crate the CPU-upload session uses: it does not expose
//! `nvEncReconfigureEncoder` (runtime bitrate), `encodePicFlags` (forced IDR, SPS/PPS
//! output), the rate-control VBV fields or the H.264 intra-refresh / `repeatSPSPPS` fields, and
//! it panics / prints on some error paths. See `adr/nvenc/0002-live-h264-session.md`.
//!
//! Only the fields this backend touches are named; every struct ends in its reserved tail so
//! the total size equals the C struct. Sizes and offsets were dumped from the SDK 13.0 header
//! (`FFmpeg` `nv-codec-headers` tag `n13.0.19.0`, `include/ffnvcodec/nvEncodeAPI.h`) with
//! `clang-cl` on `x86_64` and are pinned by the `const` assertions below — a field slip fails to
//! compile instead of corrupting the driver's view of a struct.

#![allow(
    dead_code,
    clippy::struct_field_names,
    reason = "FFI mirror: some named fields exist only to keep the C layout"
)]

use std::ffi::{c_char, c_void};

pub(super) type Status = i32;
pub(super) const NV_ENC_SUCCESS: Status = 0;
pub(super) const NV_ENC_ERR_NEED_MORE_INPUT: Status = 17;

pub(super) const NVENCAPI_MAJOR_VERSION: u32 = 13;
pub(super) const NVENCAPI_MINOR_VERSION: u32 = 0;
pub(super) const NVENCAPI_VERSION: u32 = NVENCAPI_MAJOR_VERSION | (NVENCAPI_MINOR_VERSION << 24);

const fn struct_version(ver: u32) -> u32 {
    NVENCAPI_VERSION | (ver << 16) | (0x7 << 28)
}

pub(super) const NV_ENCODE_API_FUNCTION_LIST_VER: u32 = struct_version(2);
pub(super) const NV_ENC_CAPS_PARAM_VER: u32 = struct_version(1);
pub(super) const NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS_VER: u32 = struct_version(1);
pub(super) const NV_ENC_CREATE_BITSTREAM_BUFFER_VER: u32 = struct_version(1);
pub(super) const NV_ENC_CONFIG_VER: u32 = struct_version(9) | (1 << 31);
pub(super) const NV_ENC_INITIALIZE_PARAMS_VER: u32 = struct_version(7) | (1 << 31);
pub(super) const NV_ENC_RECONFIGURE_PARAMS_VER: u32 = struct_version(2) | (1 << 31);
pub(super) const NV_ENC_PRESET_CONFIG_VER: u32 = struct_version(5) | (1 << 31);
pub(super) const NV_ENC_PIC_PARAMS_VER: u32 = struct_version(7) | (1 << 31);
pub(super) const NV_ENC_LOCK_BITSTREAM_VER: u32 = struct_version(2) | (1 << 31);
pub(super) const NV_ENC_MAP_INPUT_RESOURCE_VER: u32 = struct_version(4);
pub(super) const NV_ENC_REGISTER_RESOURCE_VER: u32 = struct_version(5);

pub(super) const NVENC_INFINITE_GOPLENGTH: u32 = 0xffff_ffff;

// `NV_ENC_DEVICE_TYPE`, `NV_ENC_INPUT_RESOURCE_TYPE`, `NV_ENC_BUFFER_USAGE`.
pub(super) const NV_ENC_DEVICE_TYPE_DIRECTX: u32 = 0;
pub(super) const NV_ENC_INPUT_RESOURCE_TYPE_DIRECTX: u32 = 0;
pub(super) const NV_ENC_INPUT_IMAGE: u32 = 0;

// `NV_ENC_BUFFER_FORMAT`. `ARGB` is word-ordered A8R8G8B8, i.e. bytes B,G,R,A in memory —
// `DXGI_FORMAT_B8G8R8A8_UNORM`. `ABGR` is bytes R,G,B,A — `DXGI_FORMAT_R8G8B8A8_UNORM`.
pub(super) const NV_ENC_BUFFER_FORMAT_NV12: u32 = 0x0000_0001;
pub(super) const NV_ENC_BUFFER_FORMAT_ARGB: u32 = 0x0100_0000;
pub(super) const NV_ENC_BUFFER_FORMAT_ABGR: u32 = 0x1000_0000;

pub(super) const NV_ENC_PARAMS_RC_CBR: u32 = 0x2;
pub(super) const NV_ENC_TUNING_INFO_LOW_LATENCY: u32 = 2;
pub(super) const NV_ENC_TUNING_INFO_ULTRA_LOW_LATENCY: u32 = 3;
pub(super) const NV_ENC_PIC_STRUCT_FRAME: u32 = 0x01;
pub(super) const NV_ENC_PIC_TYPE_I: u32 = 0x02;
pub(super) const NV_ENC_PIC_TYPE_IDR: u32 = 0x03;

// `NV_ENC_PIC_FLAGS`.
pub(super) const NV_ENC_PIC_FLAG_FORCEIDR: u32 = 0x2;
pub(super) const NV_ENC_PIC_FLAG_OUTPUT_SPSPPS: u32 = 0x4;
pub(super) const NV_ENC_PIC_FLAG_EOS: u32 = 0x8;

// `NV_ENC_CAPS` (sequential enum; values dumped from the header).
pub(super) const NV_ENC_CAPS_WIDTH_MAX: u32 = 16;
pub(super) const NV_ENC_CAPS_HEIGHT_MAX: u32 = 17;
pub(super) const NV_ENC_CAPS_SUPPORT_DYN_BITRATE_CHANGE: u32 = 20;
pub(super) const NV_ENC_CAPS_SUPPORT_INTRA_REFRESH: u32 = 25;

// `NV_ENC_VUI_*` (H.264 Annex E code points).
pub(super) const NV_ENC_VUI_VIDEO_FORMAT_UNSPECIFIED: u32 = 5;
pub(super) const NV_ENC_VUI_BT709: u32 = 1;

// `NV_ENC_RC_PARAMS` bitfield word.
pub(super) const RC_ENABLE_LOOKAHEAD: u32 = 1 << 5;
pub(super) const RC_ZERO_REORDER_DELAY: u32 = 1 << 9;

// `NV_ENC_CONFIG_H264` bitfield word.
pub(super) const H264_OUTPUT_RECOVERY_POINT_SEI: u32 = 1 << 9;
pub(super) const H264_ENABLE_INTRA_REFRESH: u32 = 1 << 10;
pub(super) const H264_REPEAT_SPSPPS: u32 = 1 << 12;

// `NV_ENC_RECONFIGURE_PARAMS` bitfield word.
pub(super) const RECONFIGURE_RESET_ENCODER: u32 = 1 << 0;
pub(super) const RECONFIGURE_FORCE_IDR: u32 = 1 << 1;

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(super) struct Guid {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

pub(super) const NV_ENC_CODEC_H264_GUID: Guid = Guid {
    data1: 0x6bc8_2762,
    data2: 0x4e63,
    data3: 0x4ca4,
    data4: [0xaa, 0x85, 0x1e, 0x50, 0xf3, 0x21, 0xf6, 0xbf],
};
pub(super) const NV_ENC_H264_PROFILE_HIGH_GUID: Guid = Guid {
    data1: 0xe7cb_c309,
    data2: 0x4f7a,
    data3: 0x4b89,
    data4: [0xaf, 0x2a, 0xd5, 0x37, 0xc9, 0x2b, 0xe3, 0x10],
};
/// `NV_ENC_PRESET_P1_GUID` .. `NV_ENC_PRESET_P7_GUID` (index 0 = P1, fastest).
pub(super) const NV_ENC_PRESET_GUIDS: [Guid; 7] = [
    Guid {
        data1: 0xfc0a_8d3e,
        data2: 0x45f8,
        data3: 0x4cf8,
        data4: [0x80, 0xc7, 0x29, 0x88, 0x71, 0x59, 0x0e, 0xbf],
    },
    Guid {
        data1: 0xf581_cfb8,
        data2: 0x88d6,
        data3: 0x4381,
        data4: [0x93, 0xf0, 0xdf, 0x13, 0xf9, 0xc2, 0x7d, 0xab],
    },
    Guid {
        data1: 0x3685_0110,
        data2: 0x3a07,
        data3: 0x441f,
        data4: [0x94, 0xd5, 0x36, 0x70, 0x63, 0x1f, 0x91, 0xf6],
    },
    Guid {
        data1: 0x90a7_b826,
        data2: 0xdf06,
        data3: 0x4862,
        data4: [0xb9, 0xd2, 0xcd, 0x6d, 0x73, 0xa0, 0x86, 0x81],
    },
    Guid {
        data1: 0x21c6_e6b4,
        data2: 0x297a,
        data3: 0x4cba,
        data4: [0x99, 0x8f, 0xb6, 0xcb, 0xde, 0x72, 0xad, 0xe3],
    },
    Guid {
        data1: 0x8e75_c279,
        data2: 0x6299,
        data3: 0x4ab6,
        data4: [0x83, 0x02, 0x0b, 0x21, 0x5a, 0x33, 0x5c, 0xf5],
    },
    Guid {
        data1: 0x8484_8c12,
        data2: 0x6f71,
        data3: 0x4c13,
        data4: [0x93, 0x1b, 0x53, 0xe2, 0x83, 0xf5, 0x79, 0x74],
    },
];

#[repr(C)]
pub(super) struct CapsParam {
    pub version: u32,
    pub caps_to_query: u32,
    pub reserved: [u32; 62],
}

#[repr(C)]
pub(super) struct OpenEncodeSessionExParams {
    pub version: u32,
    pub device_type: u32,
    pub device: *mut c_void,
    pub reserved: *mut c_void,
    pub api_version: u32,
    pub reserved1: [u32; 253],
    pub reserved2: [*mut c_void; 64],
}

#[repr(C)]
pub(super) struct CreateBitstreamBuffer {
    pub version: u32,
    pub size: u32,
    pub memory_heap: u32,
    pub reserved: u32,
    pub bitstream_buffer: *mut c_void,
    pub bitstream_buffer_ptr: *mut c_void,
    pub reserved1: [u32; 58],
    pub reserved2: [*mut c_void; 64],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct Qp {
    pub inter_p: u32,
    pub inter_b: u32,
    pub intra: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct RcParams {
    pub version: u32,
    pub rate_control_mode: u32,
    pub const_qp: Qp,
    pub average_bit_rate: u32,
    pub max_bit_rate: u32,
    pub vbv_buffer_size: u32,
    pub vbv_initial_delay: u32,
    /// `enableMinQP` .. `enableExtLookahead` bitfields — see the `RC_*` constants.
    pub flags: u32,
    pub min_qp: Qp,
    pub max_qp: Qp,
    pub initial_rc_qp: Qp,
    pub temporal_layer_idx_mask: u32,
    pub temporal_layer_qp: [u8; 8],
    pub target_quality: u8,
    pub target_quality_lsb: u8,
    pub lookahead_depth: u16,
    pub low_delay_key_frame_scale: u8,
    pub y_dc_qp_index_offset: i8,
    pub u_dc_qp_index_offset: i8,
    pub v_dc_qp_index_offset: i8,
    pub qp_map_mode: u32,
    pub multi_pass: u32,
    pub alpha_layer_bitrate_ratio: u32,
    pub cb_qp_index_offset: i8,
    pub cr_qp_index_offset: i8,
    pub reserved2: u16,
    pub lookahead_level: u32,
    pub view_bitrate_ratios: [u8; 7],
    pub reserved3: u8,
    pub reserved1: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct H264Vui {
    pub overscan_info_present_flag: u32,
    pub overscan_info: u32,
    pub video_signal_type_present_flag: u32,
    pub video_format: u32,
    pub video_full_range_flag: u32,
    pub colour_description_present_flag: u32,
    pub colour_primaries: u32,
    pub transfer_characteristics: u32,
    pub colour_matrix: u32,
    pub chroma_sample_location_flag: u32,
    pub chroma_sample_location_top: u32,
    pub chroma_sample_location_bot: u32,
    pub bitstream_restriction_flag: u32,
    pub timing_info_present_flag: u32,
    pub num_unit_in_ticks: u32,
    pub time_scale: u32,
    pub reserved: [u32; 12],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct H264Config {
    /// `enableTemporalSVC` .. `enableTimeCode` bitfields — see the `H264_*` constants.
    pub flags: u32,
    pub level: u32,
    pub idr_period: u32,
    pub separate_colour_plane_flag: u32,
    pub disable_deblocking_filter_idc: u32,
    pub num_temporal_layers: u32,
    pub sps_id: u32,
    pub pps_id: u32,
    pub adaptive_transform_mode: u32,
    pub fmo_mode: u32,
    pub bdirect_mode: u32,
    pub entropy_coding_mode: u32,
    pub stereo_mode: u32,
    pub intra_refresh_period: u32,
    pub intra_refresh_cnt: u32,
    pub max_num_ref_frames: u32,
    pub slice_mode: u32,
    pub slice_mode_data: u32,
    pub vui: H264Vui,
    pub ltr_num_frames: u32,
    pub ltr_trust_mode: u32,
    pub chroma_format_idc: u32,
    pub max_temporal_layers: u32,
    pub use_b_frames_as_ref: u32,
    pub num_ref_l0: u32,
    pub num_ref_l1: u32,
    pub output_bit_depth: u32,
    pub input_bit_depth: u32,
    pub tf_level: u32,
    pub reserved1: [u32; 264],
    pub reserved2: [*mut c_void; 64],
}

/// `NV_ENC_CODEC_CONFIG`: only the H.264 arm is named; `raw` pins the union's C size.
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) union CodecConfig {
    pub h264: H264Config,
    pub raw: [u64; 1792 / 8],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct Config {
    pub version: u32,
    pub profile_guid: Guid,
    pub gop_length: u32,
    pub frame_interval_p: i32,
    pub mono_chrome_encoding: u32,
    pub frame_field_mode: u32,
    pub mv_precision: u32,
    pub rc_params: RcParams,
    pub encode_codec_config: CodecConfig,
    pub reserved: [u32; 278],
    pub reserved2: [*mut c_void; 64],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct InitializeParams {
    pub version: u32,
    pub encode_guid: Guid,
    pub preset_guid: Guid,
    pub encode_width: u32,
    pub encode_height: u32,
    pub dar_width: u32,
    pub dar_height: u32,
    pub frame_rate_num: u32,
    pub frame_rate_den: u32,
    pub enable_encode_async: u32,
    pub enable_ptd: u32,
    pub flags: u32,
    pub priv_data_size: u32,
    pub reserved: u32,
    pub priv_data: *mut c_void,
    pub encode_config: *mut Config,
    pub max_encode_width: u32,
    pub max_encode_height: u32,
    pub max_me_hint_counts_per_block: [[u32; 4]; 2],
    pub tuning_info: u32,
    pub buffer_format: u32,
    pub num_state_buffers: u32,
    pub output_stats_level: u32,
    pub reserved1: [u32; 284],
    pub reserved2: [*mut c_void; 64],
}

#[repr(C)]
pub(super) struct ReconfigureParams {
    pub version: u32,
    pub reserved: u32,
    pub re_init_encode_params: InitializeParams,
    /// `resetEncoder` / `forceIDR` bitfields — see the `RECONFIGURE_*` constants.
    pub flags: u32,
    pub reserved2: u32,
}

#[repr(C)]
pub(super) struct PresetConfig {
    pub version: u32,
    pub reserved: u32,
    pub preset_cfg: Config,
    pub reserved1: [u32; 256],
    pub reserved2: [*mut c_void; 64],
}

#[repr(C)]
pub(super) struct PicParams {
    pub version: u32,
    pub input_width: u32,
    pub input_height: u32,
    pub input_pitch: u32,
    pub encode_pic_flags: u32,
    pub frame_idx: u32,
    pub input_time_stamp: u64,
    pub input_duration: u64,
    pub input_buffer: *mut c_void,
    pub output_bitstream: *mut c_void,
    pub completion_event: *mut c_void,
    pub buffer_fmt: u32,
    pub picture_struct: u32,
    pub picture_type: u32,
    /// `NV_ENC_CODEC_PIC_PARAMS` (left zeroed: no per-picture codec overrides).
    pub codec_pic_params: [u64; 1544 / 8],
    /// `meHintCountsPerBlock` .. `reserved6` (left zeroed).
    pub tail: [u64; (3360 - 1624) / 8],
}

#[repr(C)]
pub(super) struct LockBitstream {
    pub version: u32,
    /// `doNotWait` / `ltrFrame` / `getRCStats` bitfields.
    pub flags: u32,
    pub output_bitstream: *mut c_void,
    pub slice_offsets: *mut u32,
    pub frame_idx: u32,
    pub hw_encode_status: u32,
    pub num_slices: u32,
    pub bitstream_size_in_bytes: u32,
    pub output_time_stamp: u64,
    pub output_duration: u64,
    pub bitstream_buffer_ptr: *mut c_void,
    pub picture_type: u32,
    pub picture_struct: u32,
    pub frame_avg_qp: u32,
    pub frame_satd: u32,
    /// `ltrFrameIdx` .. `reservedInternal`.
    pub tail: [u64; (1544 - 80) / 8],
}

#[repr(C)]
pub(super) struct MapInputResource {
    pub version: u32,
    pub sub_resource_index: u32,
    pub input_resource: *mut c_void,
    pub registered_resource: *mut c_void,
    pub mapped_resource: *mut c_void,
    pub mapped_buffer_fmt: u32,
    pub reserved1: [u32; 251],
    pub reserved2: [*mut c_void; 63],
}

#[repr(C)]
pub(super) struct RegisterResource {
    pub version: u32,
    pub resource_type: u32,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub sub_resource_index: u32,
    pub resource_to_register: *mut c_void,
    pub registered_resource: *mut c_void,
    pub buffer_format: u32,
    pub buffer_usage: u32,
    pub input_fence_point: *mut c_void,
    pub chroma_offset: [u32; 2],
    pub chroma_offset_in: [u32; 2],
    pub reserved1: [u32; 244],
    pub reserved2: [*mut c_void; 61],
}

type Enc = *mut c_void;
type FnGetEncodeGuidCount = unsafe extern "system" fn(Enc, *mut u32) -> Status;
type FnGetEncodeGuids = unsafe extern "system" fn(Enc, *mut Guid, u32, *mut u32) -> Status;
type FnGetEncodeCaps = unsafe extern "system" fn(Enc, Guid, *mut CapsParam, *mut i32) -> Status;
type FnInitializeEncoder = unsafe extern "system" fn(Enc, *mut InitializeParams) -> Status;
type FnCreateBitstreamBuffer = unsafe extern "system" fn(Enc, *mut CreateBitstreamBuffer) -> Status;
type FnPtrArg = unsafe extern "system" fn(Enc, *mut c_void) -> Status;
type FnEncodePicture = unsafe extern "system" fn(Enc, *mut PicParams) -> Status;
type FnLockBitstream = unsafe extern "system" fn(Enc, *mut LockBitstream) -> Status;
type FnMapInputResource = unsafe extern "system" fn(Enc, *mut MapInputResource) -> Status;
type FnDestroyEncoder = unsafe extern "system" fn(Enc) -> Status;
type FnOpenEncodeSessionEx =
    unsafe extern "system" fn(*mut OpenEncodeSessionExParams, *mut Enc) -> Status;
type FnRegisterResource = unsafe extern "system" fn(Enc, *mut RegisterResource) -> Status;
type FnReconfigureEncoder = unsafe extern "system" fn(Enc, *mut ReconfigureParams) -> Status;
type FnGetLastErrorString = unsafe extern "system" fn(Enc) -> *const c_char;
type FnGetEncodePresetConfigEx =
    unsafe extern "system" fn(Enc, Guid, Guid, u32, *mut PresetConfig) -> Status;

/// `NV_ENCODE_API_FUNCTION_LIST`. Unused slots are `*mut c_void` placeholders; every used
/// slot is an `Option` because the driver fills the table and a null entry must not be called.
#[repr(C)]
pub(super) struct FunctionList {
    pub version: u32,
    pub reserved: u32,
    pub open_encode_session: *mut c_void,
    pub get_encode_guid_count: Option<FnGetEncodeGuidCount>,
    pub get_encode_profile_guid_count: *mut c_void,
    pub get_encode_profile_guids: *mut c_void,
    pub get_encode_guids: Option<FnGetEncodeGuids>,
    pub get_input_format_count: *mut c_void,
    pub get_input_formats: *mut c_void,
    pub get_encode_caps: Option<FnGetEncodeCaps>,
    pub get_encode_preset_count: *mut c_void,
    pub get_encode_preset_guids: *mut c_void,
    pub get_encode_preset_config: *mut c_void,
    pub initialize_encoder: Option<FnInitializeEncoder>,
    pub create_input_buffer: *mut c_void,
    pub destroy_input_buffer: *mut c_void,
    pub create_bitstream_buffer: Option<FnCreateBitstreamBuffer>,
    pub destroy_bitstream_buffer: Option<FnPtrArg>,
    pub encode_picture: Option<FnEncodePicture>,
    pub lock_bitstream: Option<FnLockBitstream>,
    pub unlock_bitstream: Option<FnPtrArg>,
    pub lock_input_buffer: *mut c_void,
    pub unlock_input_buffer: *mut c_void,
    pub get_encode_stats: *mut c_void,
    pub get_sequence_params: *mut c_void,
    pub register_async_event: *mut c_void,
    pub unregister_async_event: *mut c_void,
    pub map_input_resource: Option<FnMapInputResource>,
    pub unmap_input_resource: Option<FnPtrArg>,
    pub destroy_encoder: Option<FnDestroyEncoder>,
    pub invalidate_ref_frames: *mut c_void,
    pub open_encode_session_ex: Option<FnOpenEncodeSessionEx>,
    pub register_resource: Option<FnRegisterResource>,
    pub unregister_resource: Option<FnPtrArg>,
    pub reconfigure_encoder: Option<FnReconfigureEncoder>,
    pub reserved1: *mut c_void,
    pub create_mv_buffer: *mut c_void,
    pub destroy_mv_buffer: *mut c_void,
    pub run_motion_estimation_only: *mut c_void,
    pub get_last_error_string: Option<FnGetLastErrorString>,
    pub set_io_cuda_streams: *mut c_void,
    pub get_encode_preset_config_ex: Option<FnGetEncodePresetConfigEx>,
    pub get_sequence_param_ex: *mut c_void,
    pub restore_encoder_state: *mut c_void,
    pub lookahead_picture: *mut c_void,
    pub reserved2: [*mut c_void; 275],
}

/// `NvEncodeAPICreateInstance`.
pub(super) type FnCreateInstance = unsafe extern "system" fn(*mut FunctionList) -> Status;
/// `NvEncodeAPIGetMaxSupportedVersion`: `(major << 4) | minor`.
pub(super) type FnGetMaxSupportedVersion = unsafe extern "system" fn(*mut u32) -> Status;

/// All-zero value of a plain-old-data FFI struct (the NVENC convention: zero, then set
/// `version` and the fields you need).
pub(super) const fn zeroed<T: Pod>() -> T {
    // SAFETY: `Pod` is only implemented below for `#[repr(C)]` structs made of integers,
    // raw pointers, `Option<fn>` and arrays/unions of those, for which all-zero bytes is a
    // valid value (null pointers, `None`, `0`).
    unsafe { std::mem::zeroed() }
}

/// Marker for the FFI structs above whose all-zero bit pattern is valid.
pub(super) trait Pod {}
impl Pod for CapsParam {}
impl Pod for OpenEncodeSessionExParams {}
impl Pod for CreateBitstreamBuffer {}
impl Pod for InitializeParams {}
impl Pod for ReconfigureParams {}
impl Pod for PresetConfig {}
impl Pod for PicParams {}
impl Pod for LockBitstream {}
impl Pod for MapInputResource {}
impl Pod for RegisterResource {}
impl Pod for FunctionList {}

// Layout pins (x86_64, SDK 13.0) — see the module docs for where the numbers come from.
#[cfg(target_pointer_width = "64")]
const _: () = {
    use std::mem::{offset_of, size_of};
    assert!(size_of::<CapsParam>() == 256);
    assert!(size_of::<OpenEncodeSessionExParams>() == 1552);
    assert!(size_of::<CreateBitstreamBuffer>() == 776);
    assert!(size_of::<RcParams>() == 128);
    assert!(offset_of!(RcParams, average_bit_rate) == 20);
    assert!(offset_of!(RcParams, min_qp) == 40);
    assert!(offset_of!(RcParams, lookahead_depth) == 90);
    assert!(offset_of!(RcParams, multi_pass) == 100);
    assert!(offset_of!(RcParams, lookahead_level) == 112);
    assert!(offset_of!(RcParams, reserved1) == 124);
    assert!(size_of::<H264Vui>() == 112);
    assert!(size_of::<H264Config>() == 1792);
    assert!(offset_of!(H264Config, intra_refresh_period) == 52);
    assert!(offset_of!(H264Config, vui) == 72);
    assert!(offset_of!(H264Config, ltr_num_frames) == 184);
    assert!(offset_of!(H264Config, tf_level) == 220);
    assert!(offset_of!(H264Config, reserved2) == 1280);
    assert!(size_of::<CodecConfig>() == 1792);
    assert!(size_of::<Config>() == 3584);
    assert!(offset_of!(Config, rc_params) == 40);
    assert!(offset_of!(Config, encode_codec_config) == 168);
    assert!(offset_of!(Config, reserved2) == 3072);
    assert!(size_of::<InitializeParams>() == 1800);
    assert!(offset_of!(InitializeParams, priv_data) == 80);
    assert!(offset_of!(InitializeParams, encode_config) == 88);
    assert!(offset_of!(InitializeParams, max_me_hint_counts_per_block) == 104);
    assert!(offset_of!(InitializeParams, tuning_info) == 136);
    assert!(offset_of!(InitializeParams, reserved2) == 1288);
    assert!(size_of::<ReconfigureParams>() == 1816);
    assert!(offset_of!(ReconfigureParams, re_init_encode_params) == 8);
    assert!(offset_of!(ReconfigureParams, reserved2) == 1812);
    assert!(size_of::<PresetConfig>() == 5128);
    assert!(offset_of!(PresetConfig, preset_cfg) == 8);
    assert!(offset_of!(PresetConfig, reserved1) == 3592);
    assert!(size_of::<PicParams>() == 3360);
    assert!(offset_of!(PicParams, input_time_stamp) == 24);
    assert!(offset_of!(PicParams, input_buffer) == 40);
    assert!(offset_of!(PicParams, completion_event) == 56);
    assert!(offset_of!(PicParams, buffer_fmt) == 64);
    assert!(offset_of!(PicParams, picture_type) == 72);
    assert!(offset_of!(PicParams, codec_pic_params) == 80);
    assert!(offset_of!(PicParams, tail) == 1624);
    assert!(size_of::<LockBitstream>() == 1544);
    assert!(offset_of!(LockBitstream, output_bitstream) == 8);
    assert!(offset_of!(LockBitstream, bitstream_size_in_bytes) == 36);
    assert!(offset_of!(LockBitstream, output_time_stamp) == 40);
    assert!(offset_of!(LockBitstream, bitstream_buffer_ptr) == 56);
    assert!(offset_of!(LockBitstream, picture_type) == 64);
    assert!(offset_of!(LockBitstream, frame_avg_qp) == 72);
    assert!(offset_of!(LockBitstream, tail) == 80);
    assert!(size_of::<MapInputResource>() == 1544);
    assert!(offset_of!(MapInputResource, registered_resource) == 16);
    assert!(offset_of!(MapInputResource, mapped_buffer_fmt) == 32);
    assert!(size_of::<RegisterResource>() == 1536);
    assert!(offset_of!(RegisterResource, resource_to_register) == 24);
    assert!(offset_of!(RegisterResource, buffer_format) == 40);
    assert!(offset_of!(RegisterResource, input_fence_point) == 48);
    assert!(offset_of!(RegisterResource, chroma_offset) == 56);
    assert!(size_of::<FunctionList>() == 2552);
    assert!(offset_of!(FunctionList, get_encode_caps) == 64);
    assert!(offset_of!(FunctionList, open_encode_session_ex) == 240);
    assert!(offset_of!(FunctionList, reconfigure_encoder) == 264);
    assert!(offset_of!(FunctionList, get_last_error_string) == 304);
    assert!(offset_of!(FunctionList, get_encode_preset_config_ex) == 320);
    assert!(offset_of!(FunctionList, reserved2) == 352);
    assert!(NV_ENCODE_API_FUNCTION_LIST_VER == 1_879_179_277);
};
