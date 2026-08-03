//! Real browser API: wasm-bindgen classes over [`iso_bmff`] mux/demux.
//!
//! ADR-0020 (`@mediaway/browser` package): the WASM module owns the container —
//! muxer typestate mirror (`new` → `addTrack` → `begin` → `pushPacket` →
//! `flush` → `pollBytes`) and streaming demuxer (`pushBytes` → `streams` →
//! `pollPacket`). Codecs come from the host (`WebCodecs`); capture from native Web
//! APIs — neither lives here.
//!
//! `Uint8Array` in/out (copied at the boundary), explicit `.free()` on every
//! exported object (JS GC cannot see into wasm memory).

use bytes::Bytes;
use iso_bmff::{
    Codec, Demuxer as CoreDemuxer, Error, Live, Muxer as CoreMuxer, Open, Rational, Sample, Track,
};
use wasm_bindgen::prelude::*;

fn bmff_err(error: &Error) -> JsValue {
    JsValue::from_str(&error.to_string())
}

fn codec_to_str(codec: Codec) -> String {
    match codec {
        Codec::H264 => "h264",
        Codec::Hevc => "hevc",
        Codec::Av1 => "av1",
        Codec::Vp9 => "vp9",
        Codec::Aac => "aac",
        Codec::Opus => "opus",
        Codec::WebVtt => "webvtt",
        Codec::Tx3g => "tx3g",
        _ => "unknown",
    }
    .to_owned()
}

fn codec_from_str(codec: &str) -> Result<Codec, JsValue> {
    match codec {
        "h264" => Ok(Codec::H264),
        "hevc" => Ok(Codec::Hevc),
        "av1" => Ok(Codec::Av1),
        "vp9" => Ok(Codec::Vp9),
        "aac" => Ok(Codec::Aac),
        "opus" => Ok(Codec::Opus),
        "webvtt" => Ok(Codec::WebVtt),
        "tx3g" => Ok(Codec::Tx3g),
        _ => Err(JsValue::from_str(&format!("unknown codec: {codec}"))),
    }
}

/// Track / stream description (mirror of `iso_bmff::Track`).
///
/// Constructed with the same fields a Rust caller would pass to
/// `Muxer::addTrack`: id, lowercase codec string, timebase rational,
/// video geometry (0 for audio), and codec config bytes (`extra_data`).
#[wasm_bindgen]
pub struct JsTrack {
    track: Track,
}

#[wasm_bindgen]
impl JsTrack {
    /// Create a track description. `codec` is lowercase (`"h264"`, `"aac"`, ...).
    #[wasm_bindgen(constructor)]
    pub fn new(
        id: u32,
        codec: &str,
        time_base_num: u64,
        time_base_den: u32,
        width: u32,
        height: u32,
        extra_data: Vec<u8>,
    ) -> Result<Self, JsValue> {
        Ok(Self {
            track: Track {
                id,
                codec: codec_from_str(codec)?,
                time_base: Rational::new(time_base_num, time_base_den),
                width,
                height,
                extra_data: Bytes::from(extra_data),
            },
        })
    }

    /// 0-based track id.
    #[wasm_bindgen(getter)]
    pub fn id(&self) -> u32 {
        self.track.id
    }

    /// Lowercase codec name (`"h264"`, `"aac"`, ...).
    #[wasm_bindgen(getter)]
    pub fn codec(&self) -> String {
        codec_to_str(self.track.codec)
    }

    /// Timebase numerator.
    #[wasm_bindgen(getter)]
    pub fn time_base_num(&self) -> u64 {
        self.track.time_base.num
    }

    /// Timebase denominator (non-zero).
    #[wasm_bindgen(getter)]
    pub fn time_base_den(&self) -> u32 {
        self.track.time_base.den
    }

    /// Video width (0 for audio tracks).
    #[wasm_bindgen(getter)]
    pub fn width(&self) -> u32 {
        self.track.width
    }

    /// Video height (0 for audio tracks).
    #[wasm_bindgen(getter)]
    pub fn height(&self) -> u32 {
        self.track.height
    }

    /// Codec config bytes (e.g. `avcC` / `AudioSpecificConfig`). Fresh copy.
    #[wasm_bindgen(getter)]
    pub fn extra_data(&self) -> Vec<u8> {
        self.track.extra_data.to_vec()
    }
}

impl From<Track> for JsTrack {
    fn from(track: Track) -> Self {
        Self { track }
    }
}

/// One compressed sample (mirror of `iso_bmff::Sample`).
#[wasm_bindgen]
pub struct JsSample {
    sample: Sample,
}

#[wasm_bindgen]
impl JsSample {
    /// Create a sample. `payload` is the compressed bitstream chunk.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(constructor)]
    pub fn new(
        stream_id: u32,
        pts: i64,
        dts: i64,
        duration: u64,
        is_keyframe: bool,
        is_discard: bool,
        payload: Vec<u8>,
    ) -> Self {
        Self {
            sample: Sample {
                stream_id,
                pts,
                dts,
                duration,
                is_keyframe,
                is_discard,
                payload: Bytes::from(payload),
            },
        }
    }

    /// Track id this sample belongs to.
    #[wasm_bindgen(getter)]
    pub fn stream_id(&self) -> u32 {
        self.sample.stream_id
    }

    /// Presentation timestamp in the track's timescale.
    #[wasm_bindgen(getter)]
    pub fn pts(&self) -> i64 {
        self.sample.pts
    }

    /// Decode timestamp in the track's timescale.
    #[wasm_bindgen(getter)]
    pub fn dts(&self) -> i64 {
        self.sample.dts
    }

    /// Sample duration in the track's timescale.
    #[wasm_bindgen(getter)]
    pub fn duration(&self) -> u64 {
        self.sample.duration
    }

    /// Sync sample (keyframe).
    #[wasm_bindgen(getter)]
    pub fn is_keyframe(&self) -> bool {
        self.sample.is_keyframe
    }

    /// Outside the active edit window (decode dependency / padding).
    #[wasm_bindgen(getter)]
    pub fn is_discard(&self) -> bool {
        self.sample.is_discard
    }

    /// Compressed payload bytes. Fresh copy.
    #[wasm_bindgen(getter)]
    pub fn payload(&self) -> Vec<u8> {
        self.sample.payload.to_vec()
    }
}

impl From<Sample> for JsSample {
    fn from(sample: Sample) -> Self {
        Self { sample }
    }
}

/// Fragmented-MP4 muxer with an explicit `begin()` typestate transition.
///
/// `new(fragmentBatch)` → `addTrack` × N → `begin()` → `pushPacket` × N →
/// `flush()` → `pollBytes()`. Call `.free()` when done (wasm memory).
#[wasm_bindgen]
pub struct Muxer {
    open: Option<CoreMuxer<Open>>,
    live: Option<CoreMuxer<Live>>,
    output: Vec<u8>,
}

#[wasm_bindgen]
impl Muxer {
    /// Create a muxer. `fragment_batch` — samples per fMP4 fragment (>= 1).
    #[wasm_bindgen(constructor)]
    pub fn new(fragment_batch: u32) -> Self {
        Self {
            open: Some(CoreMuxer::with_fragment_batch(fragment_batch as usize)),
            live: None,
            output: Vec::new(),
        }
    }

    /// Register a track. Must be called before `begin()`.
    pub fn add_track(&mut self, track: &JsTrack) -> Result<u32, JsValue> {
        let mux = self
            .open
            .as_mut()
            .ok_or_else(|| JsValue::from_str("addTrack: begin() already called"))?;
        mux.add_track(track.track.clone()).map_err(|e| bmff_err(&e))
    }

    /// Lock tracks and enter the live streaming state (consumes the open state).
    pub fn begin(&mut self) -> Result<(), JsValue> {
        let open = self
            .open
            .take()
            .ok_or_else(|| JsValue::from_str("begin: already called"))?;
        self.live = Some(open.begin());
        Ok(())
    }

    /// Push one compressed sample (H.264 Annex-B is auto-converted to AVCC).
    pub fn push_packet(&mut self, sample: &JsSample) -> Result<(), JsValue> {
        let mux = self
            .live
            .as_mut()
            .ok_or_else(|| JsValue::from_str("pushPacket: call begin() first"))?;
        mux.push_packet(&sample.sample).map_err(|e| bmff_err(&e))
    }

    /// Finalize the current fragment.
    pub fn flush(&mut self) {
        if let Some(mux) = self.live.as_mut() {
            mux.flush();
        }
    }

    /// Full accumulated fMP4 output — fresh `Uint8Array` copy each call.
    pub fn poll_bytes(&mut self) -> Vec<u8> {
        if let Some(mux) = self.live.as_mut() {
            mux.poll_bytes(&mut self.output);
        }
        self.output.clone()
    }
}

/// Streaming fragmented-MP4 demuxer.
///
/// `new()` → `pushBytes` × N → `streams()` / `pollPacket()`. Call `.free()`
/// when done (wasm memory).
#[wasm_bindgen]
pub struct Demuxer {
    inner: CoreDemuxer,
}

#[wasm_bindgen]
impl Demuxer {
    /// Create an empty streaming demuxer.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: CoreDemuxer::new(),
        }
    }

    /// Feed a chunk of fMP4 bytes (streaming — call repeatedly).
    pub fn push_bytes(&mut self, bytes: &[u8]) {
        self.inner.push_bytes(bytes);
    }

    /// Demuxed track descriptions (from `moov`, available once the header is in).
    pub fn streams(&self) -> Vec<JsTrack> {
        self.inner
            .streams()
            .iter()
            .cloned()
            .map(JsTrack::from)
            .collect()
    }

    /// Next packet, or `null` when the input is exhausted.
    pub fn poll_packet(&mut self) -> Option<JsSample> {
        self.inner.poll_packet().map(JsSample::from)
    }
}

impl Default for Demuxer {
    fn default() -> Self {
        Self::new()
    }
}
