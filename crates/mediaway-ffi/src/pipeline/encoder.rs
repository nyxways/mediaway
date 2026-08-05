//! Opaque intermediate auto-encoder handle and its C ABI functions.
//!
//! Handle shape and panic-safety strategy: `adr/0001-auto-encode-c-abi.md` §3, §7.

use std::panic::{AssertUnwindSafe, catch_unwind};

use mediaway::platform::AutoEncoder;
use mediaway_common::{Packet, StreamInfo, VideoFrame};
use mediaway_encoder::EncodeError;
use mediaway_encoder::VideoEncoder;
use mediaway_encoder::auto::AutoVideoEncodeConfig;

use crate::pipeline::status::MediawayPipelineStatus;
use crate::pipeline::types::MediawayAutoVideoEncodeConfig;

/// Opaque intermediate auto-encoder handle (`mediaway_auto_encoder_t*` in the C header).
///
/// Needs no wrapper struct or `poisoned` flag: the handle *is* the trait object,
/// because its only two operations ([`mediaway_encode_session_open`] and
/// [`mediaway_auto_encoder_close`]) both destroy the pointer unconditionally, so
/// there is no repeated-call-after-panic scenario to guard against.
///
/// `#[repr(transparent)]` newtype, not a bare `Box<dyn VideoEncoder>` type alias —
/// `cbindgen` (`docs/adr/0016-cbindgen-ffi-headers.md`) can forward-declare a
/// newtype struct as an opaque C handle but has no way to do the same for a type
/// alias to a trait object. Same layout as the type alias it replaces (a boxed fat
/// pointer, 2 words) — `#[repr(transparent)]` guarantees no extra indirection.
#[repr(transparent)]
pub struct AutoEncoderHandle(Box<dyn VideoEncoder>);

impl VideoEncoder for AutoEncoderHandle {
    fn stream_info(&self) -> &StreamInfo {
        self.0.stream_info()
    }

    fn push_frame(&mut self, frame: &VideoFrame) -> Result<(), EncodeError> {
        self.0.push_frame(frame)
    }

    fn poll_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        self.0.poll_packet()
    }

    fn flush(&mut self) -> Result<(), EncodeError> {
        self.0.flush()
    }
}

/// Open the best available video encoder for `config` on the current platform.
///
/// Three outcomes: (1) `Ok` — builds the handle, writes it to `*out_encoder`; (2) a
/// normal `Err` (e.g. [`mediaway_encoder::EncodeError::NoBackend`]) — no handle
/// exists, `*out_encoder` is set to `NULL`, the matching status is returned; (3) a
/// caught panic — same `NULL`/[`MediawayPipelineStatus::InternalPanic`] shape as (2).
///
/// # Safety
///
/// `config` must be a valid, readable [`MediawayAutoVideoEncodeConfig`] pointer.
/// `out_encoder` must be a valid, writable, non-null out-parameter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_auto_encoder_open(
    config: *const MediawayAutoVideoEncodeConfig,
    out_encoder: *mut *mut AutoEncoderHandle,
) -> MediawayPipelineStatus {
    if config.is_null() || out_encoder.is_null() {
        return MediawayPipelineStatus::InvalidArgument;
    }
    // SAFETY: caller guarantees `config` is valid for reads (function contract).
    let config = unsafe { *config };
    // SAFETY: `out_encoder` is checked non-null above; caller guarantees it is
    // writable (function contract).
    unsafe { out_encoder.write(std::ptr::null_mut()) };

    let result = catch_unwind(AssertUnwindSafe(|| {
        let mut rust_config = AutoVideoEncodeConfig::new(
            config.codec.into(),
            config.width,
            config.height,
            config.time_base.into(),
        );
        rust_config.bitrate_bps = config.bitrate_bps;
        rust_config.pixel_format = config.pixel_format.into();
        rust_config.gpu_device = config.gpu_device.to_common();
        AutoEncoder::open(&rust_config)
    }));

    match result {
        Ok(Ok(encoder)) => {
            let handle: Box<AutoEncoderHandle> = Box::new(AutoEncoderHandle(encoder));
            // SAFETY: `out_encoder` is checked non-null above (function contract).
            unsafe { out_encoder.write(Box::into_raw(handle)) };
            MediawayPipelineStatus::Ok
        }
        Ok(Err(err)) => err.into(),
        Err(_) => MediawayPipelineStatus::InternalPanic,
    }
}

/// Close and free an auto-encoder handle without ever opening a session on it.
///
/// Only for abandoning an opened encoder before calling
/// [`mediaway_encode_session_open`] on it — that function consumes `encoder`
/// unconditionally, so do not call this afterward (double-free).
///
/// # Safety
///
/// `encoder` must be null or a pointer previously returned by
/// [`mediaway_auto_encoder_open`] and not already consumed by this function or
/// [`mediaway_encode_session_open`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mediaway_auto_encoder_close(encoder: *mut AutoEncoderHandle) {
    if encoder.is_null() {
        return;
    }
    // A panic during drop is deliberately swallowed and the allocation leaked — same
    // reasoning as `mediaway_muxer_close` (`adr/0001-auto-encode-c-abi.md` §7).
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: caller guarantees `encoder` is a valid, not-yet-consumed handle
        // pointer (function contract).
        drop(unsafe { Box::from_raw(encoder) });
    }));
}
