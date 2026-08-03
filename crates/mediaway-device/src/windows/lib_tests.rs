#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "unit tests"
)]

use super::*;
use crate::{DeviceHotplug, DeviceKind};

/// Opens a real `WindowsDeviceHotplug` (Microphone + Loopback), polls a few times, and
/// closes cleanly. Does **not** attempt to simulate an actual device plug/unplug event —
/// that isn't something a CI/test environment can trigger — so under normal conditions
/// every poll is expected to be idle (`Ok(None)`); a real event arriving during the
/// sleep window is logged, not treated as a failure.
#[test]
fn open_hotplug_mic_loopback_poll_or_skip() {
    let _guard = crate::windows::HARDWARE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut hotplug =
        match WindowsDeviceHotplug::open(&[DeviceKind::Microphone, DeviceKind::Loopback]) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("skip: WindowsDeviceHotplug::open ({e:?})");
                return;
            }
        };
    for _ in 0..3 {
        match hotplug.poll_event() {
            Ok(Some(event)) => eprintln!("hotplug: real event observed during poll: {event:?}"),
            Ok(None) => {}
            Err(e) => {
                eprintln!("skip: poll_event ({e:?})");
                return;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    hotplug.close().expect("close");
}

/// Empirically CONFIRMS that the real, OS-provided `MMDeviceEnumerator` does **not**
/// implement `IAgileObject` on this machine (`QueryInterface` fails with
/// `E_NOINTERFACE`) — despite this crate's own `hotplug.rs` module doc previously
/// claiming "`MMDeviceEnumerator` instances are documented free-threaded/agile COM
/// objects" (that claim was never cited to a primary source and has been corrected
/// there; see this test). The registry's `ThreadingModel=Both` for this CLSID
/// (`{BCDE0395-E52F-467C-8E3D-C4579291692E}`) is real but does **not** imply
/// `IAgileObject` support — `Both` only governs which apartment the class factory may
/// activate the object in, not whether an already-created instance may be handed to a
/// different thread without marshaling. `IAgileObject` is the interface COM itself
/// queries to decide that, so this test asks the live object directly rather than
/// inferring from the registry.
///
/// This is the empirical answer to the open question
/// `mediaway-device-ffi/adr/0002-callback-event-delivery.md` §8 flagged as "implied by
/// the type's own doc comments, not confirmed by compiling anything": **`unsafe impl
/// Send for WindowsDeviceHotplug` would be unsound** and must not be added on the
/// strength of this object alone. If a future Windows/`windows-rs` update ever adds
/// real `IAgileObject` support here, this test starts failing — that is the intended
/// signal to re-open the Send question, not a bug in the assertion below.
#[test]
fn mmdevice_enumerator_does_not_implement_iagileobject_or_skip() {
    use windows::Win32::Media::Audio::{IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, IAgileObject,
    };
    use windows::core::Interface;

    let _guard = crate::windows::HARDWARE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    // SAFETY: standard apartment init, scoped to this test via `ComGuard`
    // (matches hotplug.rs's own per-call `CoInitializeEx`/`ComGuard` idiom).
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if hr.is_err() {
        eprintln!("skip: CoInitializeEx ({hr:?})");
        return;
    }
    let _com = crate::windows_audio::ComGuard;

    // SAFETY: standard in-proc COM activation, matching hotplug.rs's own use.
    let enumerator: windows_core::Result<IMMDeviceEnumerator> =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER) };
    let enumerator = match enumerator {
        Ok(e) => e,
        Err(e) => {
            eprintln!("skip: CoCreateInstance(MMDeviceEnumerator) ({e:?})");
            return;
        }
    };

    let agile: windows_core::Result<IAgileObject> = enumerator.cast();
    assert!(
        agile.is_err(),
        "the real MMDeviceEnumerator on this machine now implements IAgileObject \
         ({agile:?}) — the environment changed since this test was written; re-open the \
         `WindowsDeviceHotplug: Send` question in \
         mediaway-device-ffi/adr/0002-callback-event-delivery.md §8 and hotplug.rs's module \
         doc instead of just updating this assertion"
    );
}
