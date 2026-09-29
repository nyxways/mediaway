//! Encoder and decoder capability probes (`adr/pipeline/0007-stream-bytes-aac-decode-support-probe.md` §3).
//!
//! **Both probes are costly.** They open throwaway sessions (a real MFT / VA-API /
//! `VideoToolbox` session per row), so call them when a settings screen opens, not per
//! frame, and never in a loop.

use std::panic::{AssertUnwindSafe, catch_unwind};

use mediaway::platform;
use mediaway_decoder::capability::{DecodeSupport, DecodeUnavailable};
use mediaway_encoder::auto::{Backend, EncodePathClass};
use mediaway_encoder::capability::{EncodeSupport, EncodeUnavailable};

use crate::pipeline::status::MediawayPipelineStatus;
use crate::pipeline::types::MediawayPipelineCodecKind;

/// Which encode backend a probe row describes. Mirrors `mediaway_encoder::auto::Backend`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediawayEncodeBackend {
    /// The platform's own media API (Media Foundation, `VideoToolbox`, VA-API).
    Os = 0,
    /// NVIDIA NVENC.
    Nvenc = 1,
    /// Intel Quick Sync Video.
    QuickSync = 2,
    /// AMD AMF.
    Amf = 3,
    /// `VK_KHR_video_encode_queue`.
    Vulkan = 4,
    /// Pure-Rust software encoder.
    Software = 5,
    /// A backend added after this ABI version.
    Unknown = 255,
}

/// Whether a backend or codec is usable right now.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediawaySupportState {
    /// Usable; for an encoder row, `path_class` says at what cost.
    Supported = 0,
    /// No code path exists for this combination on this platform.
    NotImplemented = 1,
    /// Real code exists but the driver or device did not answer right now.
    NoDevice = 2,
    /// A state added after this ABI version.
    Unknown = 255,
}

/// The cheapest data path a supported encoder reached. `None` unless the row is `Supported`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediawayEncodePathClass {
    /// The row is not `Supported`.
    None = 0,
    /// GPU handle accepted with no copy or readback.
    ZeroCopy = 1,
    /// GPU-to-GPU copy or cross-API share; no CPU round trip.
    GpuCopy = 2,
    /// CPU planes uploaded into a hardware encoder.
    CpuUpload = 3,
    /// GPU-to-CPU readback, then encode (costly).
    Readback = 4,
    /// Software encoder.
    Software = 5,
    /// A path class added after this ABI version.
    Unknown = 255,
}

/// One row of [`mediaway_encoder_support_at`] — plain value, no owned fields.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediawayEncoderCapability {
    /// Which backend this row describes.
    pub backend: MediawayEncodeBackend,
    /// Whether it is usable right now.
    pub state: MediawaySupportState,
    /// Data path cost; meaningful only when `state == Supported`.
    pub path_class: MediawayEncodePathClass,
}

const fn backend_of(backend: Backend) -> MediawayEncodeBackend {
    match backend {
        Backend::Os => MediawayEncodeBackend::Os,
        Backend::Nvenc => MediawayEncodeBackend::Nvenc,
        Backend::QuickSync => MediawayEncodeBackend::QuickSync,
        Backend::Amf => MediawayEncodeBackend::Amf,
        Backend::Vulkan => MediawayEncodeBackend::Vulkan,
        Backend::Software => MediawayEncodeBackend::Software,
        _ => MediawayEncodeBackend::Unknown,
    }
}

const fn path_class_of(path: EncodePathClass) -> MediawayEncodePathClass {
    match path {
        EncodePathClass::ZeroCopy => MediawayEncodePathClass::ZeroCopy,
        EncodePathClass::GpuCopy => MediawayEncodePathClass::GpuCopy,
        EncodePathClass::CpuUpload => MediawayEncodePathClass::CpuUpload,
        EncodePathClass::Readback => MediawayEncodePathClass::Readback,
        EncodePathClass::Software => MediawayEncodePathClass::Software,
        _ => MediawayEncodePathClass::Unknown,
    }
}

const fn encoder_row(backend: Backend, support: EncodeSupport) -> MediawayEncoderCapability {
    let (state, path_class) = match support {
        EncodeSupport::Supported(path) => (MediawaySupportState::Supported, path_class_of(path)),
        EncodeSupport::Unavailable(EncodeUnavailable::NotImplemented) => (
            MediawaySupportState::NotImplemented,
            MediawayEncodePathClass::None,
        ),
        EncodeSupport::Unavailable(EncodeUnavailable::NoDevice) => (
            MediawaySupportState::NoDevice,
            MediawayEncodePathClass::None,
        ),
        _ => (MediawaySupportState::Unknown, MediawayEncodePathClass::None),
    };
    MediawayEncoderCapability {
        backend: backend_of(backend),
        state,
        path_class,
    }
}

const fn decode_state_of(support: DecodeSupport) -> MediawaySupportState {
    match support {
        DecodeSupport::Supported => MediawaySupportState::Supported,
        DecodeSupport::Unavailable(DecodeUnavailable::NotImplemented) => {
            MediawaySupportState::NotImplemented
        }
        DecodeSupport::Unavailable(DecodeUnavailable::NoDevice) => MediawaySupportState::NoDevice,
        _ => MediawaySupportState::Unknown,
    }
}

/// Probe every encode backend for `codec` **at `width` x `height`**.
///
/// Encoder support is resolution-dependent — a hardware encoder has minimum and maximum
/// dimensions — so there is no resolution-free form. Pass the size you will encode.
///
/// **Costly:** opens a throwaway session per backend. Writes an owned array to `*out_rows`
/// and its length to `*out_count`; free it with [`mediaway_encoder_support_free`]. A platform
/// with no per-backend selection reports zero rows: `*out_rows == NULL`, `*out_count == 0`.
///
/// # Safety
///
/// `out_rows` and `out_count` must be valid, writable, non-null out-parameters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_encoder_support_at(
    codec: MediawayPipelineCodecKind,
    width: u32,
    height: u32,
    out_rows: *mut *mut MediawayEncoderCapability,
    out_count: *mut usize,
) -> MediawayPipelineStatus {
    if out_rows.is_null() || out_count.is_null() {
        return MediawayPipelineStatus::InvalidArgument;
    }
    // SAFETY: both are checked non-null above and writable per the function contract.
    unsafe {
        out_rows.write(std::ptr::null_mut());
        out_count.write(0);
    }
    if width == 0 || height == 0 {
        return MediawayPipelineStatus::InvalidInput;
    }

    let result = catch_unwind(AssertUnwindSafe(|| {
        platform::encoder_support_at(codec.into(), width, height)
            .into_iter()
            .map(|row| encoder_row(row.backend, row.support))
            .collect::<Vec<_>>()
    }));

    match result {
        Ok(rows) if rows.is_empty() => MediawayPipelineStatus::Ok,
        Ok(rows) => {
            let boxed = rows.into_boxed_slice();
            let count = boxed.len();
            let ptr = Box::into_raw(boxed).cast::<MediawayEncoderCapability>();
            // SAFETY: both are checked non-null above (function contract).
            unsafe {
                out_rows.write(ptr);
                out_count.write(count);
            }
            MediawayPipelineStatus::Ok
        }
        Err(_) => MediawayPipelineStatus::InternalPanic,
    }
}

/// Free an array returned by [`mediaway_encoder_support_at`]. Always safe to call with
/// `(NULL, 0)`.
///
/// # Safety
///
/// `rows`/`count` must be exactly the pair that function wrote, not already freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_encoder_support_free(
    rows: *mut MediawayEncoderCapability,
    count: usize,
) {
    if rows.is_null() {
        return;
    }
    // SAFETY: `rows`/`count` came from `Box::into_raw` of a boxed slice of exactly `count`
    // elements in `mediaway_encoder_support_at` (function contract).
    drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(rows, count)) });
}

/// Probe whether decoding `codec` is usable on this machine right now.
///
/// Decode has one implementation per platform, so this is one state, not a list. It is how a
/// caller learns whether AAC decode exists here (`adr/pipeline/0007` §2) without opening a
/// session. **Costly:** opens a throwaway session.
///
/// # Safety
///
/// `out_state` must be a valid, writable, non-null out-parameter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_decoder_support(
    codec: MediawayPipelineCodecKind,
    out_state: *mut MediawaySupportState,
) -> MediawayPipelineStatus {
    if out_state.is_null() {
        return MediawayPipelineStatus::InvalidArgument;
    }
    catch_unwind(AssertUnwindSafe(|| platform::decoder_support(codec.into()))).map_or(
        MediawayPipelineStatus::InternalPanic,
        |support| {
            // SAFETY: `out_state` is checked non-null above (function contract).
            unsafe { out_state.write(decode_state_of(support)) };
            MediawayPipelineStatus::Ok
        },
    )
}
