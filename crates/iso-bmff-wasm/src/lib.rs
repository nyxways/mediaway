//! WASM exports for [`iso_bmff`] mux/demux smoke tests in the browser.

#![forbid(unsafe_code)]

use bytes::Bytes;
use iso_bmff::{Codec, Demuxer, Error, Muxer, Rational, Sample, Track};
use wasm_bindgen::prelude::*;

const AVC_C: &[u8] = &[
    1, 0x42, 0, 0x1e, 0xff, 0xe1, 0, 4, 0x67, 0x42, 0x00, 0x1e, 1, 0, 4, 0x68, 0xce, 0x06, 0xe2,
];

const fn h264_track() -> Track {
    Track {
        id: 0,
        codec: Codec::H264,
        time_base: Rational::new(1, 1000),
        width: 64,
        height: 64,
        extra_data: Bytes::from_static(AVC_C),
    }
}

const fn aac_track() -> Track {
    Track {
        id: 1,
        codec: Codec::Aac,
        time_base: Rational::new(1, 48_000),
        width: 0,
        height: 0,
        extra_data: Bytes::from_static(&[0x11, 0x90]),
    }
}

const fn vp9_track() -> Track {
    Track {
        id: 0,
        codec: Codec::Vp9,
        time_base: Rational::new(1, 1000),
        width: 64,
        height: 64,
        extra_data: Bytes::new(),
    }
}

fn bmff_err(error: &Error) -> JsValue {
    JsValue::from_str(&error.to_string())
}

fn mux_minimal_av_bytes() -> Result<Vec<u8>, JsValue> {
    let mut open = Muxer::with_fragment_batch(1);
    open.add_track(h264_track()).map_err(|e| bmff_err(&e))?;
    open.add_track(aac_track()).map_err(|e| bmff_err(&e))?;
    let mut mux = open.begin();
    mux.push_packet(&Sample {
        stream_id: 0,
        pts: 0,
        dts: 0,
        duration: 33,
        is_keyframe: true,
        is_discard: false,
        payload: Bytes::from_static(&[0, 0, 0, 2, 0x65, 0x88]),
    })
    .map_err(|e| bmff_err(&e))?;
    mux.push_packet(&Sample {
        stream_id: 1,
        pts: 0,
        dts: 0,
        duration: 1024,
        is_keyframe: true,
        is_discard: false,
        payload: Bytes::from_static(&[0x21, 0x10, 0x04, 0x60, 0x8c, 0x1c]),
    })
    .map_err(|e| bmff_err(&e))?;
    mux.flush();
    let mut bytes = Vec::new();
    mux.poll_bytes(&mut bytes);
    if bytes.len() < 12 || &bytes[4..8] != b"ftyp" {
        return Err(JsValue::from_str("invalid fMP4 output"));
    }
    Ok(bytes)
}

/// Mux a minimal H.264 + AAC fMP4 fragment and return demuxed packet count.
#[wasm_bindgen]
pub fn wasm_mux_demux_smoke() -> Result<u32, JsValue> {
    let bytes = mux_minimal_av_bytes()?;
    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);
    let mut count = 0u32;
    while demux.poll_packet().is_some() {
        count += 1;
    }
    Ok(count)
}

/// Return fMP4 bytes for a minimal H.264 + AAC mux (for Playwright / manual checks).
#[wasm_bindgen]
pub fn wasm_mux_av_bytes() -> Result<Vec<u8>, JsValue> {
    mux_minimal_av_bytes()
}

fn mux_minimal_vp9_bytes() -> Result<Vec<u8>, JsValue> {
    let mut open = Muxer::with_fragment_batch(1);
    open.add_track(vp9_track()).map_err(|e| bmff_err(&e))?;
    let mut mux = open.begin();
    mux.push_packet(&Sample {
        stream_id: 0,
        pts: 0,
        dts: 0,
        duration: 33,
        is_keyframe: true,
        is_discard: false,
        payload: Bytes::from_static(&[0x82, 0x49, 0x83, 0x42]),
    })
    .map_err(|e| bmff_err(&e))?;
    mux.flush();
    let mut bytes = Vec::new();
    mux.poll_bytes(&mut bytes);
    if bytes.len() < 12 || &bytes[4..8] != b"ftyp" {
        return Err(JsValue::from_str("invalid fMP4 output"));
    }
    Ok(bytes)
}

/// Return fMP4 bytes for a minimal VP9 mux (for Playwright / manual checks) — proves the
/// `vp09`/`vpcC` sample entry (crate-local ADR-0002), not just `avc1`.
#[wasm_bindgen]
pub fn wasm_mux_vp9_bytes() -> Result<Vec<u8>, JsValue> {
    mux_minimal_vp9_bytes()
}

/// Mux a minimal VP9 fragment and return the demuxed track's codec name (`"vp9"` on success)
/// plus packet count, joined as `"<codec>:<count>"` (for Playwright / manual checks).
#[wasm_bindgen]
pub fn wasm_mux_vp9_demux_smoke() -> Result<String, JsValue> {
    let bytes = mux_minimal_vp9_bytes()?;
    let mut demux = Demuxer::new();
    demux.push_bytes(&bytes);
    let codec = demux
        .streams()
        .first()
        .map(|s| s.codec)
        .ok_or_else(|| JsValue::from_str("no demuxed track"))?;
    let codec_name = match codec {
        Codec::Vp9 => "vp9",
        Codec::H264 => "h264",
        _ => "other",
    };
    let mut count = 0u32;
    while demux.poll_packet().is_some() {
        count += 1;
    }
    Ok(format!("{codec_name}:{count}"))
}
