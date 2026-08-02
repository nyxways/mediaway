//! Browser capture — wasm32 implementation.

#![forbid(unsafe_code)]

use js_sys::{Object, Reflect};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    DisplayMediaStreamConstraints, MediaStream, MediaStreamConstraints, MediaTrackConstraints,
};

use crate::config::{DisplayCapturePreferences, UserMediaPreferences};

/// Human-readable policy string for UI / tests.
#[wasm_bindgen]
pub fn device_selection_policy() -> String {
    "Web capture requires a user gesture and browser picker; programmatic device/window IDs are not supported."
        .to_string()
}

fn media_devices() -> Result<web_sys::MediaDevices, JsValue> {
    web_sys::window()
        .ok_or_else(|| JsValue::from_str("no window"))?
        .navigator()
        .media_devices()
        .map_err(|_| JsValue::from_str("MediaDevices unavailable"))
}

fn video_track_constraints(device_id: Option<&str>) -> Result<JsValue, JsValue> {
    if let Some(id) = device_id {
        let track = MediaTrackConstraints::new();
        track.set_device_id(&JsValue::from_str(id));
        Ok(track.into())
    } else {
        Ok(JsValue::from_bool(true))
    }
}

/// Open camera and/or microphone via `getUserMedia` (user permission UI).
#[wasm_bindgen]
pub async fn open_user_media(prefs: &UserMediaPreferences) -> Result<MediaStream, JsValue> {
    let constraints = MediaStreamConstraints::new();
    if prefs.video() {
        constraints.set_video(&video_track_constraints(prefs.device_id_hint().as_deref())?);
    } else {
        constraints.set_video(&JsValue::from_bool(false));
    }
    if prefs.audio() {
        constraints.set_audio(&video_track_constraints(prefs.device_id_hint().as_deref())?);
    } else {
        constraints.set_audio(&JsValue::from_bool(false));
    }
    let promise = media_devices()?.get_user_media_with_constraints(&constraints)?;
    let stream = JsFuture::from(promise).await?;
    stream
        .dyn_into::<MediaStream>()
        .map_err(|_| JsValue::from_str("expected MediaStream"))
}

/// Open screen/window/tab capture via `getDisplayMedia` (always shows a picker).
#[wasm_bindgen]
pub async fn open_display_capture(
    prefs: &DisplayCapturePreferences,
) -> Result<MediaStream, JsValue> {
    let constraints = DisplayMediaStreamConstraints::new();
    if let Some(surface) = prefs.display_surface_hint() {
        let track = MediaTrackConstraints::new();
        let display_surface = Object::new();
        Reflect::set(
            &display_surface,
            &JsValue::from_str("ideal"),
            &JsValue::from_str(&surface),
        )?;
        Reflect::set(
            &track,
            &JsValue::from_str("displaySurface"),
            &display_surface.into(),
        )?;
        constraints.set_video(&track.into());
    } else {
        constraints.set_video(&JsValue::from_bool(true));
    }
    constraints.set_audio(&JsValue::from_bool(false));
    let promise = media_devices()?.get_display_media_with_constraints(&constraints)?;
    let stream = JsFuture::from(promise).await?;
    stream
        .dyn_into::<MediaStream>()
        .map_err(|_| JsValue::from_str("expected MediaStream"))
}

/// Returns active video track count on a stream.
#[wasm_bindgen]
pub fn media_stream_video_track_count(stream: &MediaStream) -> u32 {
    stream.get_video_tracks().length()
}
