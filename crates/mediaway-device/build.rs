//! Links `libcamera2ndk.so` on Android — `ndk-sys` bindgens the full Camera2 NDK FFI surface
//! but has no `camera` feature / `#[link(name = "camera2ndk")]` directive of its own (unlike
//! every other NDK library it wraps), a real gap found while writing
//! `src/android/camera.rs` — see `adr/android/0001-camera2-ndk-native-camera-capture.md`.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        println!("cargo:rustc-link-lib=camera2ndk");
    }
}
