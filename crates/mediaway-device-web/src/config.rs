//! Preference types shared by wasm and host stubs.

#![forbid(unsafe_code)]

use wasm_bindgen::prelude::*;

/// Optional hints for [`crate::open_display_capture`]. All fields are best-effort only.
#[wasm_bindgen]
#[derive(Default)]
pub struct DisplayCapturePreferences {
    display_surface_hint: Option<String>,
}

#[wasm_bindgen]
impl DisplayCapturePreferences {
    /// Empty preferences (browser default picker).
    #[wasm_bindgen(constructor)]
    #[allow(clippy::missing_const_for_fn, reason = "wasm_bindgen constructor")]
    pub fn new() -> Self {
        Self {
            display_surface_hint: None,
        }
    }

    /// Hint `displaySurface` when supported.
    #[wasm_bindgen(getter)]
    pub fn display_surface_hint(&self) -> Option<String> {
        // clone: wasm getter returns owned String
        self.display_surface_hint.clone()
    }

    /// Set `displaySurface` hint (`monitor` / `window` / `browser`).
    #[wasm_bindgen(setter)]
    pub fn set_display_surface_hint(&mut self, hint: Option<String>) {
        self.display_surface_hint = hint;
    }
}

/// Optional hints for camera/mic capture. `device_id` is not guaranteed.
#[wasm_bindgen]
pub struct UserMediaPreferences {
    video: bool,
    audio: bool,
    device_id_hint: Option<String>,
}

#[wasm_bindgen]
impl UserMediaPreferences {
    /// Request video and/or audio.
    #[wasm_bindgen(constructor)]
    #[allow(clippy::missing_const_for_fn, reason = "wasm_bindgen constructor")]
    pub fn new(video: bool, audio: bool) -> Self {
        Self {
            video,
            audio,
            device_id_hint: None,
        }
    }

    /// Request video track.
    #[wasm_bindgen(getter)]
    #[allow(clippy::missing_const_for_fn, reason = "wasm_bindgen getter")]
    pub fn video(&self) -> bool {
        self.video
    }

    /// Request audio track.
    #[wasm_bindgen(getter)]
    #[allow(clippy::missing_const_for_fn, reason = "wasm_bindgen getter")]
    pub fn audio(&self) -> bool {
        self.audio
    }

    /// Optional `deviceId` hint (not guaranteed).
    #[wasm_bindgen(getter)]
    pub fn device_id_hint(&self) -> Option<String> {
        // clone: wasm getter returns owned String
        self.device_id_hint.clone()
    }

    /// Set optional `deviceId` hint.
    #[wasm_bindgen(setter)]
    pub fn set_device_id_hint(&mut self, id: Option<String>) {
        self.device_id_hint = id;
    }
}
