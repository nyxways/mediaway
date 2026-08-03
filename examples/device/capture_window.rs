//! Window capture — config shape only.
//!
//! Unlike every other capture source in this workspace, `WindowsWindowCapture`
//! (`WinRT` Graphics Capture) has **no CPU-only path**: `open()` requires both
//! a live `HWND` and a caller-owned `ID3D11Device` handed in as
//! `gpu_device: Some(GpuDeviceHandle::DirectX11(handle))` — see
//! `mediaway-device-windows-desktop`'s `wgc.rs`. Obtaining either of those (window
//! enumeration, `D3D11CreateDevice`) means calling raw Win32/WinRT FFI, which
//! is `unsafe` — code this workspace's examples deliberately don't carry
//! (`unsafe_code = deny` outside FFI/platform-backend crates).
//!
//! This example shows the config shape an app needs to build and stops
//! there. For a complete, working, hardware-tested version — real foreground
//! `HWND` via `GetForegroundWindow`, real shared `ID3D11Device` via
//! `D3D11CreateDevice`, `unsafe` fully contained and `// SAFETY:`-documented —
//! see `crates/mediaway-device-windows-desktop/src/lib_tests.rs`'s
//! `open_window_capture_foreground_or_skip` test.
//!
//! Run:
//! ```text
//! cargo run --example capture_window
//! ```

#![allow(
    clippy::print_stdout,
    clippy::expect_used,
    reason = "example prints explanatory output; expect is on a compile-time-known non-zero literal"
)]

use mediaway_common::Rational;
use mediaway_device::desktop::{
    CaptureOutputPreference, DesktopCaptureSource, DesktopVideoCaptureConfig,
};

fn main() {
    println!("capture_window: this capture source needs a real HWND and a real");
    println!("ID3D11Device, both obtained through unsafe platform FFI this example");
    println!("intentionally doesn't carry — see the doc comment at the top of");
    println!("device/capture_window.rs for a pointer to the tested version.");
    println!();
    println!("The config shape your app would build looks like this:");
    println!(
        "  DesktopVideoCaptureConfig {{ source: DesktopCaptureSource::Window {{ window }}, \
         time_base: {:?}, output: {:?}, gpu_device: Some(..) }}",
        Rational::new(1, 30),
        CaptureOutputPreference::ZeroCopyGpu,
    );
    // Constructed (not opened) so the compiler still checks the shape above stays accurate.
    let _ = DesktopVideoCaptureConfig {
        source: DesktopCaptureSource::Window {
            window: mediaway_common::NativeHandle::new(1).expect("non-zero placeholder"),
        },
        time_base: Rational::new(1, 30),
        output: CaptureOutputPreference::ZeroCopyGpu,
        gpu_device: None,
    };
}
