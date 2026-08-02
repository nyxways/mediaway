//! Host-target stubs for workspace `cargo test`.

#![forbid(unsafe_code)]
#![allow(
    clippy::unused_async,
    reason = "wasm_bindgen async exports must match wasm API"
)]

use wasm_bindgen::prelude::*;

use crate::config::{DisplayCapturePreferences, UserMediaPreferences};

/// Human-readable policy string for UI / tests.
#[wasm_bindgen]
pub fn device_selection_policy() -> String {
    "Web capture requires a user gesture and browser picker; programmatic device/window IDs are not supported."
        .to_string()
}

/// Host build: browser APIs unavailable.
#[wasm_bindgen]
pub async fn open_user_media(_prefs: &UserMediaPreferences) -> Result<(), JsValue> {
    Err(JsValue::from_str("wasm32 browser only"))
}

/// Host build: browser APIs unavailable.
#[wasm_bindgen]
pub async fn open_display_capture(_prefs: &DisplayCapturePreferences) -> Result<(), JsValue> {
    Err(JsValue::from_str("wasm32 browser only"))
}

/// Host build: no stream.
#[wasm_bindgen]
#[allow(clippy::missing_const_for_fn, reason = "wasm_bindgen export")]
pub fn media_stream_video_track_count() -> u32 {
    0
}
