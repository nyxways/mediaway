//! `VTCompressionSession` CPU-upload encode session — H.264/HEVC/`ProRes`.
//!
//! See [ADR-0001](../../adr/apple/0001-videotoolbox-h264-cpu-upload.md) for the original H.264
//! scope (Constrained-Baseline-class / CPU NV12 upload only / best-effort key-frame-interval),
//! [ADR-0002](../../adr/apple/0002-videotoolbox-hevc-encode.md) for the HEVC addition (Main-
//! class profile) and VP9/AV1's permanent non-support (no `VideoToolbox` compression API exists
//! for either), and [ADR-0006](../../adr/apple/0006-videotoolbox-prores-encode.md) for the six
//! `ProRes` profiles (all-intra, no `ProfileLevel`/GOP/bitrate properties apply) and `ProRes` RAW's
//! own permanent non-support. All three carry the zero-compile-verification caveat for this crate
//! as authored.

use std::collections::VecDeque;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::{Arc, Mutex, OnceLock};

use crate::{EncodeError, VideoEncoder, VideoEncoderConfig, VideoInputPreference};
use mediaway_common::{
    Bytes, CodecKind, ColorRange, GpuBufferHandle, Packet, PixelFormat, StreamInfo, VideoFrame,
    VideoFrameStorage, VideoGeometry,
};

use objc2_core_foundation::{
    CFArray, CFBoolean, CFDictionary, CFNumber, CFNumberType, CFRetained, CFString, CFType,
    kCFBooleanFalse, kCFBooleanTrue,
};
use objc2_core_media::{CMSampleBuffer, CMTime, kCMSampleAttachmentKey_NotSync, kCMTimeIndefinite};
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferCreateWithPlanarBytes,
    kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
    kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
};
use objc2_video_toolbox::{
    VTCompressionSession, VTEncodeInfoFlags, VTSessionSetProperty,
    kVTCompressionPropertyKey_AllowFrameReordering, kVTCompressionPropertyKey_AverageBitRate,
    kVTCompressionPropertyKey_ExpectedFrameRate, kVTCompressionPropertyKey_MaxKeyFrameInterval,
    kVTCompressionPropertyKey_ProfileLevel, kVTCompressionPropertyKey_RealTime,
    kVTProfileLevel_H264_ConstrainedBaseline_AutoLevel, kVTProfileLevel_HEVC_Main_AutoLevel,
};

use super::codec::codec_type;
use super::extradata;

/// `OSStatus`/`CVReturn` "no error" value (both use the plain C convention `0 == success`).
const NO_ERROR: i32 = 0;

/// State shared between [`VideoToolboxVideoEncoder`] and the `VTCompressionOutputCallback`,
/// which fires asynchronously on a VideoToolbox-internal thread — see ADR-0001 § Callback
/// design.
struct SharedState {
    pending: Mutex<VecDeque<Packet>>,
    /// Set once, from the callback, after the first successfully decoded SPS/PPS(+VPS) pair —
    /// [`VideoToolboxVideoEncoder::stream_info`] prefers this over `base_info` once present.
    finalized_info: OnceLock<StreamInfo>,
    base_info: StreamInfo,
    time_base_den: u32,
    /// Selects [`extradata::extract_h264`] vs. [`extradata::extract_hevc`] in `handle_output`.
    codec: CodecKind,
}

/// `VTCompressionSession` encode session (best-effort sync-frame cadence) — H.264 or HEVC
/// depending on [`SharedState::codec`]; CPU NV12 upload or Zero-Copy `CVPixelBuffer` input
/// depending on [`Self::input`] (see [ADR-0003](../../adr/apple/0003-videotoolbox-metal-zero-copy-encode.md)).
pub(crate) struct VideoToolboxVideoEncoder {
    session: CFRetained<VTCompressionSession>,
    shared: Arc<SharedState>,
    /// The extra `Arc::into_raw` strong count passed as `output_callback_ref_con` — reclaimed
    /// exactly once in `Drop`, after `invalidate()`.
    refcon_ptr: *const SharedState,
    input: VideoInputPreference,
    width: u32,
    height: u32,
    yuv420_bytes: usize,
    color_range: ColorRange,
    flushed: bool,
}

// SAFETY: `VTCompressionSession`/`CFRetained` are Core Foundation objects, which Apple documents
// as safe to use from any thread as long as access is externally synchronized (this type's
// `&mut self` API surface already enforces that for the Rust side); the only shared mutable
// state reachable from another thread is `SharedState`, which uses `Mutex`/`OnceLock`
// internally.
#[allow(
    clippy::non_send_fields_in_send_ty,
    reason = "CFRetained<VTCompressionSession> is a Core Foundation object — see the SAFETY comment above"
)]
unsafe impl Send for VideoToolboxVideoEncoder {}

impl VideoToolboxVideoEncoder {
    /// Open according to [`VideoEncoderConfig::input`].
    pub(crate) fn open(config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        validate(config)?;
        match config.input {
            VideoInputPreference::CpuUploadOk | VideoInputPreference::ZeroCopyGpu => {
                Self::open_session(config)
            }
            // `VideoInputPreference` is `#[non_exhaustive]` — an unmatched future variant is a
            // real "we don't know this input path" case, not reachable today.
            _ => Err(EncodeError::Unsupported),
        }
    }

    fn open_session(config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        let codec = codec_type(config.codec)?;
        let width = i32::try_from(config.width).map_err(|_| EncodeError::InvalidInput)?;
        let height = i32::try_from(config.height).map_err(|_| EncodeError::InvalidInput)?;

        let shared = Arc::new(SharedState {
            pending: Mutex::new(VecDeque::new()),
            finalized_info: OnceLock::new(),
            base_info: stream_info_from(config),
            time_base_den: config.time_base.den,
            codec: config.codec,
        });
        // Extra strong count handed to VideoToolbox as the callback's `refCon` — reclaimed once
        // in `Drop`, see that impl and ADR-0001 § Callback design.
        let refcon_ptr = Arc::into_raw(Arc::clone(&shared));

        let mut session_ptr: *mut VTCompressionSession = std::ptr::null_mut();
        // SAFETY: `output_callback` is a real `extern "C-unwind" fn` matching
        // `VTCompressionOutputCallback`'s exact signature; `refcon_ptr` is a valid, live pointer
        // for at least the session's lifetime (reclaimed only in `Drop`, after `invalidate()`);
        // `session_ptr` starts null.
        let status = unsafe {
            VTCompressionSession::create(
                None,
                width,
                height,
                codec,
                None,
                None,
                None,
                Some(compression_output_callback),
                refcon_ptr.cast::<c_void>().cast_mut(),
                NonNull::from(&mut session_ptr),
            )
        };
        if status != NO_ERROR {
            // SAFETY: reclaims the extra strong count taken above; no callback can have fired
            // since the session was never successfully created.
            drop(unsafe { Arc::from_raw(refcon_ptr) });
            return Err(EncodeError::Backend);
        }
        let Some(session_ptr) = NonNull::new(session_ptr) else {
            // SAFETY: same reasoning as the failure branch above.
            drop(unsafe { Arc::from_raw(refcon_ptr) });
            return Err(EncodeError::Backend);
        };
        // SAFETY: `session_ptr` is a valid, non-null pointer VideoToolbox returned alongside a
        // `NO_ERROR` status — `VTCompressionSessionCreate`'s Create Rule guarantees this carries
        // a +1 owned reference, which `CFRetained::from_raw` takes ownership of.
        let session = unsafe { CFRetained::from_raw(session_ptr) };

        if let Err(e) = configure_properties(&session, config) {
            // SAFETY: same reasoning as the earlier failure branches — no callback can have
            // fired via a session whose configuration never completed successfully.
            unsafe { session.invalidate() };
            drop(unsafe { Arc::from_raw(refcon_ptr) });
            return Err(e);
        }

        let yuv420_bytes = yuv420_size(config.width, config.height)?;

        Ok(Self {
            session,
            shared,
            refcon_ptr,
            input: config.input,
            width: config.width,
            height: config.height,
            yuv420_bytes,
            color_range: config.color_range,
            flushed: false,
        })
    }
}

/// One `CVPixelBuffer` reference for `encode_frame`, from either input path — owned (CPU
/// upload, freed when this value drops) or borrowed (Zero-Copy, the caller's own buffer, never
/// touched beyond this reference's lifetime). Avoids duplicating the `encode_frame` call site
/// per branch in [`VideoToolboxVideoEncoder::push_frame`].
enum PixelBufferRef<'a> {
    Owned(CFRetained<CVPixelBuffer>),
    Borrowed(&'a CVPixelBuffer),
}

impl AsRef<CVPixelBuffer> for PixelBufferRef<'_> {
    fn as_ref(&self) -> &CVPixelBuffer {
        match self {
            Self::Owned(buffer) => buffer,
            Self::Borrowed(buffer) => buffer,
        }
    }
}

impl VideoEncoder for VideoToolboxVideoEncoder {
    fn stream_info(&self) -> &StreamInfo {
        self.shared
            .finalized_info
            .get()
            .unwrap_or(&self.shared.base_info)
    }

    fn push_frame(&mut self, frame: &VideoFrame) -> Result<(), EncodeError> {
        if self.flushed {
            return Err(EncodeError::Closed);
        }
        if frame.width != self.width || frame.height != self.height {
            return Err(EncodeError::InvalidInput);
        }

        let pixel_buffer = match &frame.storage {
            VideoFrameStorage::Cpu { data } => {
                if self.input != VideoInputPreference::CpuUploadOk {
                    return Err(EncodeError::InvalidInput);
                }
                if data.len() < self.yuv420_bytes {
                    return Err(EncodeError::InvalidInput);
                }
                PixelBufferRef::Owned(upload_cpu_nv12(
                    data,
                    self.width,
                    self.height,
                    self.color_range,
                )?)
            }
            VideoFrameStorage::Gpu(GpuBufferHandle::Metal { buffer }) => {
                if self.input != VideoInputPreference::ZeroCopyGpu {
                    return Err(EncodeError::InvalidInput);
                }
                let ptr = NonNull::new(buffer.get() as *mut CVPixelBuffer)
                    .ok_or(EncodeError::InvalidInput)?;
                // SAFETY: `buffer` carries a caller-supplied `CVPixelBufferRef`'s raw bits — per
                // `GpuBufferHandle::Metal`'s documented convention (see ADR-0003), the caller
                // guarantees this pointer is a valid, live `CVPixelBuffer` for at least the
                // duration of this call. This backend never retains, releases, or mutates it — a
                // pure borrow, the same "opaque bits, caller owns the lifetime" contract every
                // other `GpuBufferHandle` variant already documents.
                let pixel_buffer = unsafe { ptr.as_ref() };
                if pixel_buffer.width() != self.width as usize
                    || pixel_buffer.height() != self.height as usize
                {
                    return Err(EncodeError::InvalidInput);
                }
                PixelBufferRef::Borrowed(pixel_buffer)
            }
            VideoFrameStorage::Gpu(_) => return Err(EncodeError::Unsupported),
        };

        let pts = cmtime_from_pts(frame.pts, self.shared.time_base_den);

        // SAFETY: `pixel_buffer.as_ref()` is a valid `CVPixelBuffer` for the duration of this
        // call — either freshly created (CPU-upload path) or the caller's own, borrowed per its
        // documented contract (Zero-Copy path, see above). `source_frame_refcon`/`info_flags_out`
        // are intentionally unused (both `null_mut`) — this backend recovers timing from the
        // output `CMSampleBuffer` itself (see `handle_output`), not a threaded-through refcon.
        let status = unsafe {
            self.session.encode_frame(
                pixel_buffer.as_ref(),
                pts,
                kCMTimeIndefinite,
                None,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if status != NO_ERROR {
            return Err(EncodeError::Backend);
        }
        Ok(())
    }

    fn poll_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        let mut pending = self
            .shared
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(pending.pop_front())
    }

    fn flush(&mut self) -> Result<(), EncodeError> {
        if self.flushed {
            return Ok(());
        }
        // SAFETY: `session` is a valid, still-open session (guarded by `self.flushed`).
        // `complete_frames`'s own doc comment: "all pending frames will be emitted before the
        // function returns" — this is this backend's real, synchronous drain point.
        let status = unsafe { self.session.complete_frames(kCMTimeIndefinite) };
        self.flushed = true;
        if status != NO_ERROR {
            return Err(EncodeError::Backend);
        }
        Ok(())
    }
}

impl Drop for VideoToolboxVideoEncoder {
    fn drop(&mut self) {
        if !self.flushed {
            // SAFETY: draining before invalidate — see ADR-0001 § Decisions confirmed with the
            // user (defensive mitigation for the unconfirmed invalidate/callback-cutoff
            // ordering guarantee).
            let _ = unsafe { self.session.complete_frames(kCMTimeIndefinite) };
        }
        // SAFETY: `session` is a valid, owned `CFRetained<VTCompressionSession>` about to be
        // dropped; `invalidate()` is Apple's documented deterministic-teardown call, meant to be
        // followed by releasing the last reference (which the `CFRetained` drop glue below does).
        unsafe { self.session.invalidate() };
        // SAFETY: `refcon_ptr` is exactly the `Arc::into_raw` pointer taken in `open_cpu` and
        // handed to VideoToolbox as the callback's `refCon`; reclaimed here exactly once, after
        // `invalidate()` (see the ordering caveat above) — no other code path frees it.
        drop(unsafe { Arc::from_raw(self.refcon_ptr) });
    }
}

/// `VTCompressionOutputCallback` — fires on a VideoToolbox-internal thread, decoupled from the
/// `push_frame`/`encode_frame` call that produced it. See ADR-0001 § Callback design.
unsafe extern "C-unwind" fn compression_output_callback(
    output_callback_ref_con: *mut c_void,
    _source_frame_ref_con: *mut c_void,
    status: i32,
    _info_flags: VTEncodeInfoFlags,
    sample_buffer: *mut CMSampleBuffer,
) {
    if status != NO_ERROR || sample_buffer.is_null() {
        return;
    }
    // SAFETY: `output_callback_ref_con` is exactly the `Arc::into_raw::<SharedState>` pointer
    // passed at session creation, reclaimed only in `Drop` after `invalidate()` — valid for the
    // whole lifetime any callback can fire in. Borrowed here, never re-owned (`Arc::from_raw` is
    // never called inside this function — see `Drop`'s own comment for the single reclaim site).
    let shared = unsafe { &*(output_callback_ref_con.cast::<SharedState>()) };
    // SAFETY: non-null (checked above); VideoToolbox guarantees a valid `CMSampleBuffer` for the
    // duration of this callback invocation per `VTCompressionOutputCallback`'s contract.
    let sample_buffer = unsafe { &*sample_buffer };
    handle_output(shared, sample_buffer);
}

fn handle_output(shared: &SharedState, sample_buffer: &CMSampleBuffer) {
    if shared.finalized_info.get().is_none() {
        // ProRes has no VPS/SPS/PPS-style parameter sets to extract — every ProRes sample is a
        // fully self-contained frame (see `adr/apple/0006-videotoolbox-prores-encode.md`), so
        // `base_info`'s already-empty `extra_data` is immediately final; skip straight to
        // marking it finalized rather than calling `extract_h264` against a format description
        // that has nothing H.264-shaped in it (a real bug this ProRes work also fixed: the prior
        // `_ => extract_h264(..)` catch-all here would have silently misapplied H.264 extraction
        // to any future non-H.264/HEVC codec, not just ProRes).
        if super::codec::is_prores(shared.codec) {
            let _ = shared.finalized_info.set(shared.base_info.clone());
        } else {
            // SAFETY: `sample_buffer` is a valid, callback-scoped `CMSampleBuffer` reference.
            let extracted = unsafe { sample_buffer.format_description() }.and_then(|format_desc| {
                match shared.codec {
                    CodecKind::Hevc => extradata::extract_hevc(&format_desc),
                    CodecKind::H264 => extradata::extract_h264(&format_desc),
                    // `is_supported_video_codec` already restricts `open()` to H.264/HEVC/
                    // ProRes (ProRes takes the branch above); nothing else reaches here.
                    _ => None,
                }
            });
            if let Some(extra_data) = extracted {
                let mut info = shared.base_info.clone();
                if let StreamInfo::Video { extra_data: ed, .. } = &mut info {
                    *ed = extra_data;
                }
                let _ = shared.finalized_info.set(info);
            }
        }
    }

    // SAFETY: `sample_buffer` is a valid, callback-scoped `CMSampleBuffer` reference.
    let Some(block_buffer) = (unsafe { sample_buffer.data_buffer() }) else {
        return;
    };
    // SAFETY: `block_buffer` is a valid, retained `CMBlockBuffer` just obtained above.
    let len = unsafe { block_buffer.data_length() };
    if len == 0 {
        return;
    }
    let mut payload = vec![0u8; len];
    let Some(dest) = NonNull::new(payload.as_mut_ptr().cast::<c_void>()) else {
        return;
    };
    // SAFETY: `dest` points at `len` freshly allocated, writable bytes (`payload`, sized above).
    let status = unsafe { block_buffer.copy_data_bytes(0, len, dest) };
    if status != NO_ERROR {
        return;
    }

    // SAFETY: `sample_buffer` is a valid, callback-scoped `CMSampleBuffer` reference.
    let pts_cmtime = unsafe { sample_buffer.presentation_time_stamp() };
    let pts = cmtime_to_pts(pts_cmtime, shared.time_base_den);

    let is_keyframe = is_sync_sample(sample_buffer);

    let mut pending = shared
        .pending
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    pending.push_back(Packet {
        stream_id: 0,
        pts,
        dts: pts,
        duration: 0,
        is_keyframe,
        is_discard: false,
        payload: Bytes::from(payload),
    });
}

/// Real per-sample sync-frame (IDR) detection via `kCMSampleAttachmentKey_NotSync` — replaces
/// the earlier `packet_index == 0` heuristic this ADR originally shipped with (see ADR-0001
/// addendum). Per Apple's own documented convention: the key's **absence** from the sample's
/// attachments dictionary means the sample **is** a sync sample (keyframe); presence with a
/// `true` value means it is not. A missing attachments array or dictionary — which `VideoToolbox`
/// documents as legal when there is nothing to attach — is therefore treated the same as an
/// absent key: a sync sample, not silently treated as "not a keyframe."
fn is_sync_sample(sample_buffer: &CMSampleBuffer) -> bool {
    // SAFETY: `sample_buffer` is a valid, callback-scoped `CMSampleBuffer` reference;
    // `create_if_necessary: false` never allocates or mutates, only reads an already-existing
    // attachments array if VideoToolbox populated one for this sample.
    let Some(attachments) = (unsafe { sample_buffer.sample_attachments_array(false) }) else {
        return true;
    };
    // SAFETY: `CMSampleBufferGetSampleAttachmentsArray` returns one `CFDictionary` per sample
    // (Apple's documented contract); `objc2-core-media` 0.3.2's generated binding only types
    // this as the untyped `CFArray<Opaque>` because bindgen cannot infer the element type —
    // reinterpret it as the concrete element type the ABI actually provides.
    let attachments: &CFArray<CFDictionary<CFString, CFType>> =
        unsafe { attachments.cast_unchecked() };
    // This backend only ever submits one sample per `CMSampleBuffer` (never a batch — see
    // ADR-0001 § Session lifecycle), so the attachments array (one dictionary per sample) has
    // at most one entry.
    let Some(dict) = attachments.get(0) else {
        return true;
    };
    // SAFETY: reading a real `extern "C"` static `CFString` singleton, valid for the process's
    // lifetime — the same pattern this module already uses for `kCFBooleanTrue`/`False` above.
    let key = unsafe { kCMSampleAttachmentKey_NotSync };
    let Some(not_sync) = dict.get(key) else {
        return true;
    };
    !not_sync
        .downcast_ref::<CFBoolean>()
        .is_some_and(CFBoolean::as_bool)
}

/// Copy CPU NV12 bytes into a fresh `CVPixelBuffer` (`CVPixelBufferCreateWithPlanarBytes`,
/// planar Y/UV base addresses into an owned, heap-allocated copy of `data`, released exactly
/// once by `VideoToolbox` via `release_planar_bytes`) — a genuine CPU→driver copy, named to match
/// the Windows/Linux/Android `upload_cpu_*` cost-disclosure convention.
///
/// `color_range` selects `kCVPixelFormatType_420YpCbCr8BiPlanar{Video,Full}Range` — see
/// ADR-0001 § Decisions confirmed with the user.
fn upload_cpu_nv12(
    data: &[u8],
    width: u32,
    height: u32,
    color_range: ColorRange,
) -> Result<CFRetained<CVPixelBuffer>, EncodeError> {
    let owned: Box<Vec<u8>> = Box::new(data.to_vec());
    let y_len = (width as usize) * (height as usize);
    let base_ptr = owned.as_ptr();
    // SAFETY: `base_ptr` is valid for `owned.len()` bytes (`owned` is kept alive by the
    // `Box::into_raw` handoff below, reclaimed exactly once by `release_planar_bytes`).
    let y_plane = base_ptr.cast_mut().cast::<c_void>();
    // SAFETY: `y_len <= owned.len()` (checked by the caller's `data.len() >= yuv420_bytes`
    // guard before this function is called) — offsetting within the same allocation.
    let uv_plane = unsafe { base_ptr.add(y_len) }.cast_mut().cast::<c_void>();

    let mut plane_base_address = [y_plane, uv_plane];
    let mut plane_width = [width as usize, (width / 2) as usize];
    let mut plane_height = [height as usize, (height / 2) as usize];
    let mut plane_bytes_per_row = [width as usize, width as usize];

    // `ColorRange` is `#[non_exhaustive]` (declared in a different crate) — an unmatched future
    // variant is a real "we don't know this range" case, not reachable today.
    let pixel_format_type = match color_range {
        ColorRange::Video => kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
        ColorRange::Full => kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
        _ => return Err(EncodeError::Unsupported),
    };

    let release_ref_con = Box::into_raw(owned).cast::<c_void>();
    let mut pixel_buffer_ptr: *mut CVPixelBuffer = std::ptr::null_mut();

    let Some(plane_base_address_ptr) = NonNull::new(plane_base_address.as_mut_ptr()) else {
        // SAFETY: reclaims the box handed to VideoToolbox above — creation never reached the
        // point where VideoToolbox could take ownership of it.
        drop(unsafe { Box::from_raw(release_ref_con.cast::<Vec<u8>>()) });
        return Err(EncodeError::Backend);
    };
    let Some(plane_width_ptr) = NonNull::new(plane_width.as_mut_ptr()) else {
        drop(unsafe { Box::from_raw(release_ref_con.cast::<Vec<u8>>()) });
        return Err(EncodeError::Backend);
    };
    let Some(plane_height_ptr) = NonNull::new(plane_height.as_mut_ptr()) else {
        drop(unsafe { Box::from_raw(release_ref_con.cast::<Vec<u8>>()) });
        return Err(EncodeError::Backend);
    };
    let Some(plane_bytes_per_row_ptr) = NonNull::new(plane_bytes_per_row.as_mut_ptr()) else {
        drop(unsafe { Box::from_raw(release_ref_con.cast::<Vec<u8>>()) });
        return Err(EncodeError::Backend);
    };

    // SAFETY: all plane array pointers point at 2-element stack arrays matching
    // `number_of_planes = 2`; `release_callback` is a real `extern "C-unwind" fn` matching
    // `CVPixelBufferReleasePlanarBytesCallback`'s exact signature; `release_ref_con` is the
    // `Box::into_raw` pointer this function just took, reclaimed exactly once by that callback;
    // `pixel_buffer_ptr` starts null as required.
    let cv_return = unsafe {
        CVPixelBufferCreateWithPlanarBytes(
            None,
            width as usize,
            height as usize,
            pixel_format_type,
            std::ptr::null_mut(),
            0,
            2,
            plane_base_address_ptr,
            plane_width_ptr,
            plane_height_ptr,
            plane_bytes_per_row_ptr,
            Some(release_planar_bytes),
            release_ref_con,
            None,
            NonNull::from(&mut pixel_buffer_ptr),
        )
    };

    if let (NO_ERROR, Some(pixel_buffer_ptr)) = (cv_return, NonNull::new(pixel_buffer_ptr)) {
        // SAFETY: `pixel_buffer_ptr` is a valid, non-null pointer returned alongside a
        // `NO_ERROR` status — `CVPixelBufferCreateWithPlanarBytes`'s Create Rule guarantees
        // this carries a +1 owned reference, which `CFRetained::from_raw` takes ownership of.
        Ok(unsafe { CFRetained::from_raw(pixel_buffer_ptr) })
    } else {
        // SAFETY: creation failed before VideoToolbox could take ownership of the box (the
        // release callback is never invoked on a `CVPixelBufferCreateWithPlanarBytes`
        // failure) — reclaim it here instead, the only path that would otherwise leak it.
        drop(unsafe { Box::from_raw(release_ref_con.cast::<Vec<u8>>()) });
        Err(EncodeError::Backend)
    }
}

/// SAFETY: `release_ref_con` is exactly the `Box::into_raw(Box::new(Vec<u8>))` pointer
/// `upload_cpu_nv12` passed as `release_ref_con` to `CVPixelBufferCreateWithPlanarBytes` —
/// `VideoToolbox` calls this callback exactly once, when the pixel buffer's retain count reaches
/// zero, and never otherwise touches `release_ref_con`.
unsafe extern "C-unwind" fn release_planar_bytes(
    release_ref_con: *mut c_void,
    _data_ptr: *const c_void,
    _data_size: usize,
    _number_of_planes: usize,
    _plane_addresses: *mut *const c_void,
) {
    drop(unsafe { Box::from_raw(release_ref_con.cast::<Vec<u8>>()) });
}

fn configure_properties(
    session: &VTCompressionSession,
    config: &VideoEncoderConfig,
) -> Result<(), EncodeError> {
    // SAFETY (all `set_*_property` calls below, and the `profile_level` static reads for
    // non-ProRes codecs further down): `session` is a freshly created, not-yet-started
    // `VTCompressionSession`; every property key/value passed is a confirmed-real
    // `&'static CFString` from `objc2_video_toolbox`'s generated `VTCompressionProperties`
    // bindings — reading any of them (they are `extern "C" static`s, not functions) requires
    // `unsafe` per E0133, which this block satisfies for all of them.
    unsafe {
        set_bool_property(session, kVTCompressionPropertyKey_RealTime, true)?;
        set_bool_property(
            session,
            kVTCompressionPropertyKey_AllowFrameReordering,
            false,
        )?;
        let frame_rate = frame_rate_hint(config.time_base);
        set_i32_property(
            session,
            kVTCompressionPropertyKey_ExpectedFrameRate,
            frame_rate,
        )?;

        // ProRes: no `ProfileLevel` (zero `kVTProfileLevel_ProRes*` constants exist anywhere in
        // the generated bindings — the profile is fully encoded in `codec_type`'s six distinct
        // `kCMVideoCodecType_AppleProRes*` values, unlike H.264/HEVC's separate profile-level
        // property under one shared codec type), no `MaxKeyFrameInterval` (unconditionally
        // all-intra — `config.gop_size` is silently not honored, documented on
        // `VideoEncoderConfig::gop_size`'s own rustdoc contract for backends that can't apply it),
        // no `AverageBitRate` (profile determines quality/bitrate, not a settable property —
        // `config.bitrate_bps` is silently not honored, same documented-fallback contract). See
        // `adr/apple/0006-videotoolbox-prores-encode.md`.
        if !super::codec::is_prores(config.codec) {
            // `CodecKind` is `#[non_exhaustive]` (declared in a different crate) — an unmatched
            // future variant is a real "we don't know this profile" case, not reachable today
            // (`codec_type`, called before this function in `open_session`, already rejects
            // anything but H.264/HEVC/ProRes).
            let profile_level = match config.codec {
                CodecKind::Hevc => kVTProfileLevel_HEVC_Main_AutoLevel,
                _ => kVTProfileLevel_H264_ConstrainedBaseline_AutoLevel,
            };
            set_string_property(
                session,
                kVTCompressionPropertyKey_ProfileLevel,
                profile_level,
            )?;
            let max_key_frame_interval = i32::try_from(config.gop_size.max(1)).unwrap_or(1);
            set_i32_property(
                session,
                kVTCompressionPropertyKey_MaxKeyFrameInterval,
                max_key_frame_interval,
            )?;
            if config.bitrate_bps > 0 {
                let bitrate = i32::try_from(config.bitrate_bps).unwrap_or(i32::MAX);
                set_i32_property(session, kVTCompressionPropertyKey_AverageBitRate, bitrate)?;
            }
        }
    }
    Ok(())
}

/// # Safety
///
/// `session` must be a valid `VTCompressionSession`; `key` must be a valid `&'static CFString`.
unsafe fn set_i32_property(
    session: &VTCompressionSession,
    key: &CFString,
    value: i32,
) -> Result<(), EncodeError> {
    // SAFETY: `&value` is a valid stack `i32` matching `CFNumberType::SInt32Type`'s contract.
    let number =
        unsafe { CFNumber::new(None, CFNumberType::SInt32Type, (&raw const value).cast()) };
    let number = number.ok_or(EncodeError::Backend)?;
    // SAFETY: `session` derefs to `&VTSession` (`= CFType`) per CoreFoundation toll-free
    // bridging; `number` is a valid, just-created `CFNumber`.
    let status = unsafe { VTSessionSetProperty(session, key, Some(&number)) };
    if status == NO_ERROR {
        Ok(())
    } else {
        Err(EncodeError::Backend)
    }
}

/// # Safety
///
/// `session` must be a valid `VTCompressionSession`; `key` must be a valid `&'static CFString`.
unsafe fn set_bool_property(
    session: &VTCompressionSession,
    key: &CFString,
    value: bool,
) -> Result<(), EncodeError> {
    // SAFETY: reading a real `extern "C"` static CFBoolean singleton — already `Option`-typed
    // (nullable), so this is passed through as-is rather than re-wrapped in `Some(..)`.
    let cf_bool = unsafe {
        if value {
            kCFBooleanTrue
        } else {
            kCFBooleanFalse
        }
    }
    .map(|b: &CFBoolean| -> &CFType { b });
    // SAFETY: `session` derefs to `&VTSession` per CF toll-free bridging; `cf_bool` is a valid
    // static singleton.
    let status = unsafe { VTSessionSetProperty(session, key, cf_bool) };
    if status == NO_ERROR {
        Ok(())
    } else {
        Err(EncodeError::Backend)
    }
}

/// # Safety
///
/// `session` must be a valid `VTCompressionSession`; `key`/`value` must be valid
/// `&'static CFString`s.
unsafe fn set_string_property(
    session: &VTCompressionSession,
    key: &CFString,
    value: &CFString,
) -> Result<(), EncodeError> {
    // SAFETY: `session` derefs to `&VTSession` per CF toll-free bridging; `key`/`value` are
    // valid static `CFString`s.
    let status = unsafe { VTSessionSetProperty(session, key, Some(value)) };
    if status == NO_ERROR {
        Ok(())
    } else {
        Err(EncodeError::Backend)
    }
}

/// Nearest-integer frames-per-second hint for `kVTCompressionPropertyKey_ExpectedFrameRate` —
/// a pacing hint only, not used for output packet timing (packets carry the encoder's own
/// `presentation_time_stamp` — see [`cmtime_to_pts`]).
fn frame_rate_hint(time_base: mediaway_common::Rational) -> i32 {
    if time_base.num == 0 {
        return 30;
    }
    i32::try_from(u64::from(time_base.den) / time_base.num)
        .unwrap_or(30)
        .max(1)
}

/// Build the input `CMTime` for `encode_frame` from `pts` (in `time_base_den` ticks) — mirrors
/// how Linux/Android treat `frame.pts` as a direct tick count in the caller's timebase.
fn cmtime_from_pts(pts: i64, time_base_den: u32) -> CMTime {
    CMTime {
        value: pts,
        timescale: i32::try_from(time_base_den).unwrap_or(i32::MAX),
        flags: objc2_core_media::CMTimeFlags::Valid,
        epoch: 0,
    }
}

/// Rescale a `CMTime` (the encoder's own returned timescale, which `VideoToolbox` is free to pick
/// independently of what [`cmtime_from_pts`] requested) back into `time_base_den` ticks.
fn cmtime_to_pts(time: CMTime, time_base_den: u32) -> i64 {
    if time.timescale == 0 {
        return 0;
    }
    (time.value.saturating_mul(i64::from(time_base_den))) / i64::from(time.timescale)
}

fn validate(config: &VideoEncoderConfig) -> Result<(), EncodeError> {
    if !super::codec::is_supported_video_codec(config.codec) {
        return Err(EncodeError::Unsupported);
    }
    if config.width == 0 || config.height == 0 {
        return Err(EncodeError::InvalidInput);
    }
    if config.pixel_format != PixelFormat::Nv12 {
        return Err(EncodeError::Unsupported);
    }
    if config.time_base.den == 0 {
        return Err(EncodeError::InvalidInput);
    }
    Ok(())
}

fn yuv420_size(width: u32, height: u32) -> Result<usize, EncodeError> {
    let w = usize::try_from(width).map_err(|_| EncodeError::InvalidInput)?;
    let h = usize::try_from(height).map_err(|_| EncodeError::InvalidInput)?;
    w.checked_mul(h)
        .and_then(|y| y.checked_mul(3))
        .and_then(|v| v.checked_div(2))
        .ok_or(EncodeError::InvalidInput)
}

#[allow(clippy::missing_const_for_fn, reason = "StreamInfo holds Bytes")]
fn stream_info_from(config: &VideoEncoderConfig) -> StreamInfo {
    StreamInfo::Video {
        id: 0,
        codec: config.codec,
        time_base: config.time_base,
        geometry: VideoGeometry {
            width: config.width,
            height: config.height,
        },
        extra_data: Bytes::new(),
    }
}

#[cfg(test)]
#[path = "video_tests.rs"]
mod tests;
