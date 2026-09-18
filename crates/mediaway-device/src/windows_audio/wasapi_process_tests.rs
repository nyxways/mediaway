#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::panic,
    reason = "unit tests"
)]

use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};

use super::{
    PROCESS_LOOPBACK_CHANNELS, PROCESS_LOOPBACK_RATE, open_process_loopback_client,
    process_loopback_mode,
};
use crate::CaptureError;
use crate::windows_audio::{ComGuard, HARDWARE_TEST_LOCK, WasapiProcessTreeScope};
use windows::Win32::Media::Audio::{
    PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
    PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
};

/// `AUDCLNT_E_INVALID_STREAM_FLAG`. The `windows` crate does not export a constant for it,
/// so the bit pattern is spelled out here — it is the whole point of the assertion below.
const AUDCLNT_E_INVALID_STREAM_FLAG: i32 = 0x8889_0021_u32.cast_signed();

/// The mapping bug this pins was silent: `ProcessOnly` (now
/// [`WasapiProcessTreeScope::ExcludeProcessTree`]) selected `EXCLUDE_TARGET_PROCESS_TREE`
/// while its docs promised "only the target process", so the session opened and recorded
/// every *other* process. Nothing failed; only the audio was wrong.
#[test]
fn include_children_selects_the_include_target_process_tree_mode() {
    assert_eq!(
        process_loopback_mode(WasapiProcessTreeScope::IncludeChildren),
        PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE
    );
}

#[test]
fn exclude_process_tree_selects_the_exclude_target_process_tree_mode() {
    assert_eq!(
        process_loopback_mode(WasapiProcessTreeScope::ExcludeProcessTree),
        PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE
    );
}

/// Opens a real process-loopback capture against this test process.
///
/// Not `#[ignore]`d: per `docs/conventions/testing.md` § Tests that manipulate the desktop,
/// the bar is *touching input or the screen*, and this only activates a virtual audio
/// device and stops it again. No audio needs to be playing — an open, silent stream is what
/// is being asserted.
///
/// Machines without process-loopback support (pre-Windows-10-2004) skip honestly, but even
/// there the assertion below still runs: `AUDCLNT_E_INVALID_STREAM_FLAG` means *we* called
/// `Initialize` wrong, which no Windows version can excuse. That distinction is only
/// possible because [`CaptureError::BackendCode`] carries the `HRESULT`; with the bare
/// `CaptureError::Backend` this module used to return, this test could not tell the two
/// apart and the defect stayed invisible.
#[test]
fn process_loopback_opens_for_this_process() {
    let _guard = HARDWARE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // SAFETY: COM init for this test thread; `_com` runs CoUninitialize on drop.
    // `ActivateAudioInterfaceAsync` requires a COM-initialized calling thread.
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    assert!(hr.is_ok(), "CoInitializeEx failed: {hr:?}");
    let _com = ComGuard;

    match open_process_loopback_client(std::process::id(), WasapiProcessTreeScope::IncludeChildren)
    {
        Ok((client, _capture, rate, channels)) => {
            assert_eq!(rate, PROCESS_LOOPBACK_RATE);
            assert_eq!(channels, PROCESS_LOOPBACK_CHANNELS);
            // SAFETY: Stop mirrors the Start `open_process_loopback_client` already issued.
            let _ = unsafe { client.Stop() };
        }
        Err(CaptureError::BackendCode { code }) => {
            assert_ne!(
                code, AUDCLNT_E_INVALID_STREAM_FLAG,
                "Initialize rejected our stream flags (AUDCLNT_E_INVALID_STREAM_FLAG): \
                 process loopback needs AUDCLNT_STREAMFLAGS_LOOPBACK and rejects the \
                 sample-rate-conversion flags"
            );
            eprintln!(
                "skipping: this machine has no process-loopback support ({code:#010x}); \
                 the stream-flag assertion above still held"
            );
        }
        Err(other) => panic!("process loopback failed without a native code: {other}"),
    }
}
