//! Per-process WASAPI loopback (`ActivateAudioInterfaceAsync`).

#![allow(unsafe_code)]
#![allow(
    clippy::inline_always,
    clippy::ref_as_ptr,
    clippy::redundant_pub_crate,
    reason = "windows `#[implement]` expansion + private module visibility"
)]

use std::mem::{ManuallyDrop, size_of};
use std::pin::Pin;
use std::sync::{Arc, Condvar, Mutex};

use crate::CaptureError;
use crate::windows_audio::WasapiProcessTreeScope;
use windows::Win32::Media::Audio::{
    AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, AUDIOCLIENT_ACTIVATION_PARAMS,
    AUDIOCLIENT_ACTIVATION_PARAMS_0, AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
    AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS, ActivateAudioInterfaceAsync,
    IActivateAudioInterfaceAsyncOperation, IActivateAudioInterfaceCompletionHandler,
    IActivateAudioInterfaceCompletionHandler_Impl, IAudioCaptureClient, IAudioClient,
    PROCESS_LOOPBACK_MODE, PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
    PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE, VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
    WAVEFORMATEX, WAVEFORMATEXTENSIBLE, WAVEFORMATEXTENSIBLE_0,
};
use windows::Win32::System::Com::BLOB;
use windows::Win32::System::Com::StructuredStorage::{
    PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
};
use windows::Win32::System::Variant::VT_BLOB;
use windows_core::{GUID, Interface, Ref, implement};

/// Fixed layout for process loopback (`GetMixFormat` is unsupported in this mode).
const PROCESS_LOOPBACK_RATE: u32 = 48_000;
const PROCESS_LOOPBACK_CHANNELS: u16 = 2;

#[implement(IActivateAudioInterfaceCompletionHandler)]
struct ActivateDone(Arc<(Mutex<bool>, Condvar)>);

impl IActivateAudioInterfaceCompletionHandler_Impl for ActivateDone_Impl {
    fn ActivateCompleted(
        &self,
        _op: Ref<'_, IActivateAudioInterfaceAsyncOperation>,
    ) -> windows_core::Result<()> {
        let (lock, cvar) = &*self.0;
        let Ok(mut done) = lock.lock() else {
            return Ok(());
        };
        *done = true;
        drop(done);
        cvar.notify_one();
        Ok(())
    }
}

/// Every WASAPI/COM failure in this module surfaces the `HRESULT` the OS returned.
///
/// A bare [`CaptureError::Backend`] here cost real debugging time: `Initialize` rejected
/// this module's stream flags with `AUDCLNT_E_INVALID_STREAM_FLAG` (`0x8889_0021`) on every
/// machine, and the discarded code was the only thing that named the defect.
const fn backend_code(error: &windows_core::Error) -> CaptureError {
    CaptureError::BackendCode {
        code: error.code().0,
    }
}

/// Activate + initialize a process-loopback capture client. `pub`: reused by
/// `crate::windows::capabilities` (process-loopback support probe).
///
/// # Errors
///
/// [`CaptureError::BackendCode`] carrying the failing `HRESULT` — including
/// `AUDCLNT_E_DEVICE_INVALIDATED`/`CO_E_NOTINITIALIZED` when the caller's thread has no
/// COM apartment, and activation failures on Windows releases older than 10 2004.
pub fn open_process_loopback_client(
    process_id: u32,
    tree_scope: WasapiProcessTreeScope,
) -> Result<(IAudioClient, IAudioCaptureClient, u32, u16), CaptureError> {
    let audio_client = activate_process_loopback(process_id, tree_scope)?;
    let format = float_stereo_48k();
    // `AUDCLNT_STREAMFLAGS_LOOPBACK` is mandatory for the process-loopback virtual device,
    // and it is the *only* flag this mode accepts: the sample-rate-conversion flags
    // (`AUTOCONVERTPCM` | `SRC_DEFAULT_QUALITY`) make `Initialize` fail with
    // `AUDCLNT_E_INVALID_STREAM_FLAG` (`0x8889_0021`), because there is no mix format to
    // convert from — the caller hands the engine `float_stereo_48k()` outright.
    let flags = AUDCLNT_STREAMFLAGS_LOOPBACK;
    // SAFETY: `format` lives for the Initialize call; process-loopback uses shared capture.
    unsafe {
        audio_client
            .Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                flags,
                200_000,
                0,
                std::ptr::from_ref(&format.Format),
                None,
            )
            .map_err(|e| backend_code(&e))?;
    }
    let capture: IAudioCaptureClient =
        unsafe { audio_client.GetService() }.map_err(|e| backend_code(&e))?;
    unsafe { audio_client.Start() }.map_err(|e| backend_code(&e))?;
    // Keep `format` alive past Initialize (Copy type; stack storage).
    let _ = format.Format.nSamplesPerSec;
    Ok((
        audio_client,
        capture,
        PROCESS_LOOPBACK_RATE,
        PROCESS_LOOPBACK_CHANNELS,
    ))
}

/// The two modes Windows offers, and the only two — there is no "this process but not its
/// children" mode, which is why [`WasapiProcessTreeScope`] has no such variant.
///
/// Pinned by `wasapi_process_tests.rs`: this mapping was inverted once
/// (`ExcludeProcessTree`'s predecessor was documented as "the target process only" while
/// selecting `EXCLUDE_TARGET_PROCESS_TREE`, i.e. everything *but* the target), and the
/// inversion is invisible at runtime — the session opens and records the wrong audio.
const fn process_loopback_mode(tree_scope: WasapiProcessTreeScope) -> PROCESS_LOOPBACK_MODE {
    match tree_scope {
        WasapiProcessTreeScope::IncludeChildren => {
            PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE
        }
        WasapiProcessTreeScope::ExcludeProcessTree => {
            PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE
        }
    }
}

fn activate_process_loopback(
    process_id: u32,
    tree_scope: WasapiProcessTreeScope,
) -> Result<IAudioClient, CaptureError> {
    let mode = process_loopback_mode(tree_scope);
    let mut activation = AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
            ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                TargetProcessId: process_id,
                ProcessLoopbackMode: mode,
            },
        },
    };
    let pinned = Pin::new(&mut activation);
    let cb_size = u32::try_from(size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>())
        .map_err(|_| CaptureError::Backend)?;
    // SAFETY: PROPVARIANT blob points at stack activation params for the async call.
    let raw_prop = PROPVARIANT {
        Anonymous: PROPVARIANT_0 {
            Anonymous: ManuallyDrop::new(PROPVARIANT_0_0 {
                vt: VT_BLOB,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: PROPVARIANT_0_0_0 {
                    blob: BLOB {
                        cbSize: cb_size,
                        pBlobData: std::ptr::from_mut(pinned.get_mut()).cast::<u8>(),
                    },
                },
            }),
        },
    };
    // Do not Drop PROPVARIANT — blob is not heap-allocated.
    let activation_prop = ManuallyDrop::new(raw_prop);
    let pinned_prop = Pin::new(&*activation_prop);
    let activation_params = Some(std::ptr::from_ref::<PROPVARIANT>(pinned_prop.get_ref()));

    let setup = Arc::new((Mutex::new(false), Condvar::new()));
    // clone: Arc share with COM completion handler
    let callback: IActivateAudioInterfaceCompletionHandler =
        ActivateDone(Arc::clone(&setup)).into();

    // SAFETY: ActivateAudioInterfaceAsync with process-loopback virtual device path.
    let operation = unsafe {
        ActivateAudioInterfaceAsync(
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
            &IAudioClient::IID,
            activation_params,
            &callback,
        )
    }
    .map_err(|e| backend_code(&e))?;

    {
        let (lock, cvar) = &*setup;
        let Ok(mut completed) = lock.lock() else {
            return Err(CaptureError::Backend);
        };
        while !*completed {
            completed = cvar.wait(completed).map_err(|_| CaptureError::Backend)?;
        }
    }

    let mut audio_unknown = None;
    let mut result = windows_core::HRESULT(0);
    // SAFETY: GetActivateResult out-params after completion signaled.
    unsafe { operation.GetActivateResult(&raw mut result, &raw mut audio_unknown) }
        .map_err(|e| backend_code(&e))?;
    // `result` is the activation's own HRESULT — the one that says "this Windows build has
    // no process-loopback virtual device", distinct from the out-call's HRESULT above.
    result.ok().map_err(|e| backend_code(&e))?;
    let audio_unknown = audio_unknown.ok_or(CaptureError::Backend)?;
    audio_unknown
        .cast::<IAudioClient>()
        .map_err(|e| backend_code(&e))
}

fn float_stereo_48k() -> WAVEFORMATEXTENSIBLE {
    const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
    const KSDATAFORMAT_SUBTYPE_IEEE_FLOAT: GUID =
        GUID::from_u128(0x0000_0003_0000_0010_8000_00aa_0038_9b71);
    WAVEFORMATEXTENSIBLE {
        Format: WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_EXTENSIBLE,
            nChannels: PROCESS_LOOPBACK_CHANNELS,
            nSamplesPerSec: PROCESS_LOOPBACK_RATE,
            nAvgBytesPerSec: PROCESS_LOOPBACK_RATE * u32::from(PROCESS_LOOPBACK_CHANNELS) * 4,
            nBlockAlign: PROCESS_LOOPBACK_CHANNELS * 4,
            wBitsPerSample: 32,
            cbSize: 22,
        },
        Samples: WAVEFORMATEXTENSIBLE_0 {
            wValidBitsPerSample: 32,
        },
        dwChannelMask: 0x3,
        SubFormat: KSDATAFORMAT_SUBTYPE_IEEE_FLOAT,
    }
}

#[cfg(test)]
#[path = "wasapi_process_tests.rs"]
mod tests;
