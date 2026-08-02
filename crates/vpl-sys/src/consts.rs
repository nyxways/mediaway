//! Hand-transcribed oneVPL numeric constants.
//!
//! Every value here is copied verbatim from the vendored headers under
//! `vendor/api/vpl/` (pinned commit — see `vendor/README.md`), cited by file
//! and (approximate) line. These are plain C `enum { NAME = value, ... };`
//! blocks that are **not** attached to a named/typedef'd type `bindgen` would
//! pick up under this crate's `allowlist_type("_?mfx.*")` filter (see
//! `build.rs`), so they are transcribed by hand instead of generated — a
//! small, closed, independently-checkable set.

use crate::raw::mfxStatus;

// ---- mfxdefs.h: mfxStatus (status/error codes) ----------------------------
// Only the subset this crate's Stage 1 CPU-upload encode path actually checks.

/// No error. (`mfxdefs.h`)
pub const MFX_ERR_NONE: mfxStatus = 0;
/// Unknown error. (`mfxdefs.h`)
pub const MFX_ERR_UNKNOWN: mfxStatus = -1;
/// Null pointer. (`mfxdefs.h`)
pub const MFX_ERR_NULL_PTR: mfxStatus = -2;
/// Unsupported feature. (`mfxdefs.h`)
pub const MFX_ERR_UNSUPPORTED: mfxStatus = -3;
/// Failed to allocate memory. (`mfxdefs.h`)
pub const MFX_ERR_MEMORY_ALLOC: mfxStatus = -4;
/// Insufficient buffer at input/output. (`mfxdefs.h`)
pub const MFX_ERR_NOT_ENOUGH_BUFFER: mfxStatus = -5;
/// Invalid handle. (`mfxdefs.h`)
pub const MFX_ERR_INVALID_HANDLE: mfxStatus = -6;
/// Member function called before initialization. (`mfxdefs.h`)
pub const MFX_ERR_NOT_INITIALIZED: mfxStatus = -8;
/// Expect more data at input — not a hard failure; the caller has not fed
/// enough frames yet for a packet to be ready. (`mfxdefs.h`)
pub const MFX_ERR_MORE_DATA: mfxStatus = -10;
/// Expect more surface at output. (`mfxdefs.h`)
pub const MFX_ERR_MORE_SURFACE: mfxStatus = -11;
/// Lost the hardware acceleration device. (`mfxdefs.h`)
pub const MFX_ERR_DEVICE_LOST: mfxStatus = -13;
/// Incompatible video parameters. (`mfxdefs.h`)
pub const MFX_ERR_INCOMPATIBLE_VIDEO_PARAM: mfxStatus = -14;
/// Invalid video parameters. (`mfxdefs.h`)
pub const MFX_ERR_INVALID_VIDEO_PARAM: mfxStatus = -15;
/// Device operation failure caused by GPU hang. (`mfxdefs.h`)
pub const MFX_ERR_GPU_HANG: mfxStatus = -21;
/// The previous asynchronous operation is in execution — not a hard failure.
/// (`mfxdefs.h`)
pub const MFX_WRN_IN_EXECUTION: mfxStatus = 1;
/// The hardware acceleration device is busy — retry. (`mfxdefs.h`)
pub const MFX_WRN_DEVICE_BUSY: mfxStatus = 2;
/// Software acceleration is used (no real HW path) — the caller should treat
/// this as a degraded-but-successful `Init`, not a failure. (`mfxdefs.h`)
pub const MFX_WRN_PARTIAL_ACCELERATION: mfxStatus = 4;
/// Incompatible video parameters (non-fatal — the library adjusted them).
/// (`mfxdefs.h`)
pub const MFX_WRN_INCOMPATIBLE_VIDEO_PARAM: mfxStatus = 5;

/// `true` for any `MFX_ERR_*`/`MFX_WRN_*` value this crate treats as "the call
/// itself succeeded" (`MFX_ERR_NONE` or a positive `MFX_WRN_*` warning code).
#[must_use]
pub const fn mfx_succeeded(status: mfxStatus) -> bool {
    status >= MFX_ERR_NONE
}

// ---- mfxcommon.h: mfxIMPL (implementation selector) ------------------------
// `typedef mfxI32 mfxIMPL;` values, `mfxcommon.h` ~line 100-121.

/// A single specific hardware implementation. (`mfxcommon.h`)
///
/// **Do not pass this alone to `MFXInitEx` against a directly-loaded
/// implementation library** (this crate's MVP dispatcher, bypassing the real
/// oneVPL dispatcher — see `dispatcher` module docs): hardware-confirmed on
/// this workspace's Intel UHD 770 to return `MFX_ERR_UNSUPPORTED` from
/// `libmfxhw64.dll`. Use [`MFX_IMPL_HARDWARE_ANY`] instead — see that
/// constant's docs for why.
pub const MFX_IMPL_HARDWARE: i32 = 0x0002;
/// Any hardware implementation, unconstrained — **this crate's session-open
/// implementation selector**. (`mfxcommon.h`)
///
/// Hardware-confirmed on this workspace's Intel UHD 770
/// (`libmfxhw64.dll`/`libvpl.dll`'s `MFXInitEx` compat shim, both tried): the
/// "any" wildcard (`0x0004`) succeeds where the specific-adapter selector
/// [`MFX_IMPL_HARDWARE`] (`0x0002`) returns `MFX_ERR_UNSUPPORTED`, with or
/// without [`MFX_IMPL_VIA_D3D11`] additionally OR'd in — plausibly because
/// the real oneVPL dispatcher normally resolves "the specific hardware
/// adapter" *before* calling into an implementation library, a resolution
/// step this crate's MVP dispatcher does not perform (see `dispatcher`
/// module docs). Not documented anywhere upstream; discovered empirically by
/// sweeping every `MFX_IMPL_*`/`MFX_IMPL_VIA_*` combination against real
/// hardware (`vpl-sys/src/dispatcher_tests.rs`'s hardware-gated test).
pub const MFX_IMPL_HARDWARE_ANY: i32 = 0x0004;
/// Acceleration-mode bit: use `Direct3D11` for hardware acceleration.
/// (`mfxcommon.h`)
///
/// OR into [`MFX_IMPL_HARDWARE_ANY`] for `MFXInitEx`. Hardware-confirmed:
/// **optional** for session creation itself (`MFX_IMPL_HARDWARE_ANY` alone
/// also succeeds on this workspace's Intel UHD 770) but kept explicit for
/// this crate's Stage 1 CPU-upload path since the runtime's internal
/// hardware encode block always goes through a D3D11 acceleration context on
/// Windows regardless of whether input surfaces are system- or video-memory.
pub const MFX_IMPL_VIA_D3D11: i32 = 0x0300;

// ---- mfxstructures.h: ColorFourCC / CodecFormatFourCC ----------------------
// `#define MFX_MAKEFOURCC(A,B,C,D) ((int)A + ((int)B<<8) + ((int)C<<16) + ((int)D<<24))`
// (`mfxcommon.h` line 20). Computed here via the identical formula, not pasted
// as a magic number, so the derivation stays checkable against the header.

const fn make_fourcc(a: u8, b: u8, c: u8, d: u8) -> u32 {
    (a as u32) | ((b as u32) << 8) | ((c as u32) << 16) | ((d as u32) << 24)
}

/// AVC / H.264 codec ID. (`mfxstructures.h` line ~1142)
pub const MFX_CODEC_AVC: u32 = make_fourcc(b'A', b'V', b'C', b' ');
/// HEVC / H.265 codec ID. (`mfxstructures.h` line 1143)
pub const MFX_CODEC_HEVC: u32 = make_fourcc(b'H', b'E', b'V', b'C');
/// AV1 codec ID. (`mfxstructures.h` line 1148)
pub const MFX_CODEC_AV1: u32 = make_fourcc(b'A', b'V', b'1', b' ');
/// NV12 color format. (`mfxstructures.h` line ~162)
pub const MFX_FOURCC_NV12: u32 = make_fourcc(b'N', b'V', b'1', b'2');

// ---- mfxstructures.h: PicStruct / ChromaFormat -----------------------------

/// Progressive picture. (`mfxstructures.h` line 247)
pub const MFX_PICSTRUCT_PROGRESSIVE: u16 = 0x01;
/// 4:2:0 chroma sampling. (`mfxstructures.h` line 265)
pub const MFX_CHROMAFORMAT_YUV420: u16 = 1;

// ---- mfxstructures.h: CodecProfile / CodecLevel (AVC) ----------------------

/// AVC Baseline profile. (`mfxstructures.h` line 1172)
pub const MFX_PROFILE_AVC_BASELINE: u16 = 66;
/// AVC level 4.1 (covers up to 1080p30-ish; ample for this stage's test sizes).
/// (`mfxstructures.h` line 1207)
pub const MFX_LEVEL_AVC_41: u16 = 41;

// ---- mfxstructures.h: CodecProfile / CodecLevel (HEVC) ---------------------

/// HEVC Main (8-bit 4:2:0) profile. (`mfxstructures.h` line 1263)
pub const MFX_PROFILE_HEVC_MAIN: u16 = 1;
/// HEVC level 4.1 — matches this crate's AVC 4.1 choice, ample for this
/// stage's test sizes. (`mfxstructures.h` line 1278)
pub const MFX_LEVEL_HEVC_41: u16 = 41;

// ---- mfxstructures.h: CodecProfile / CodecLevel (AV1) ----------------------

/// AV1 Main profile. (`mfxstructures.h` line 1304)
pub const MFX_PROFILE_AV1_MAIN: u16 = 1;
/// AV1 level 4.1. (`mfxstructures.h` line 1320)
pub const MFX_LEVEL_AV1_41: u16 = 41;

// ---- mfxstructures.h: TargetUsage ------------------------------------------

/// Balanced quality/speed target usage (`MFX_TARGETUSAGE_4`). (`mfxstructures.h`
/// line ~1396-1406)
pub const MFX_TARGETUSAGE_BALANCED: u16 = 4;

// ---- mfxstructures.h: RateControlMethod ------------------------------------

/// Constant bitrate. (`mfxstructures.h` line 1412)
pub const MFX_RATECONTROL_CBR: u16 = 1;
/// Constant QP. (`mfxstructures.h` line 1414)
pub const MFX_RATECONTROL_CQP: u16 = 3;

// ---- mfxstructures.h: IOPattern ---------------------------------------------

/// Input is a linear buffer directly in system memory (CPU-upload path — this
/// crate's only Stage 1 path). (`mfxstructures.h` line 1135)
pub const MFX_IOPATTERN_IN_SYSTEM_MEMORY: u16 = 0x02;

// ---- mfxstructures.h: FrameType flags --------------------------------------

/// I-frame. (`mfxstructures.h` line 2984)
pub const MFX_FRAMETYPE_I: u16 = 0x0001;
/// Reference frame. (`mfxstructures.h` line 2989)
pub const MFX_FRAMETYPE_REF: u16 = 0x0040;
/// IDR frame. (`mfxstructures.h` line 2990)
pub const MFX_FRAMETYPE_IDR: u16 = 0x0080;

// ---- mfxstructures.h: mfxHandleType -----------------------------------------

/// Pointer to `ID3D11Device`.
///
/// **Not used by this crate's Stage 1 CPU-upload path**; kept for the
/// documented future Zero-Copy D3D11 work (see
/// `mediaway-encoder-quicksync/adr/0001`). (`mfxstructures.h` line 437)
pub const MFX_HANDLE_D3D11_DEVICE: u32 = 3;

// ---- mfxcommon.h: GPUCopy -----------------------------------------------------

/// Default GPU-copy hinting (let the runtime decide). (`mfxcommon.h` line 190)
pub const MFX_GPUCOPY_DEFAULT: u16 = 0;
