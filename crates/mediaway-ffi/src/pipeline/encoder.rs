//! Opaque intermediate auto-encoder handle and its C ABI functions.
//!
//! Handle shape and panic-safety strategy: `adr/0001-auto-encode-c-abi.md` §3, §7.

use std::panic::{AssertUnwindSafe, catch_unwind};

use mediaway::platform::AutoEncoder;
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
/// `*mut AutoEncoderHandle` is a thin C pointer to a heap-allocated `Box<dyn
/// VideoEncoder>` (itself a fat pointer, 2 words) — i.e. `Box::new(fat_ptr)`
/// boxes an already-`Sized` value, giving a normal thin allocation. Not a
/// double-`Box::into_raw` of the same allocation; one extra level of
/// indirection versus a concrete `Box<T: Sized>` handle.
pub type AutoEncoderHandle = Box<dyn VideoEncoder>;

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
            let handle: Box<AutoEncoderHandle> = Box::new(encoder);
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
