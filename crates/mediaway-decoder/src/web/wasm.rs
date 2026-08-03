//! `WebCodecs` decode backend — wasm32 browser implementation.

#![forbid(unsafe_code)]

use std::cell::RefCell;
use std::rc::Rc;

use js_sys::Uint8Array;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    EncodedVideoChunk, EncodedVideoChunkInit, EncodedVideoChunkType, PlaneLayout, VideoDecoder,
    VideoDecoderConfig as WebVideoDecoderConfig, VideoDecoderInit, VideoFrame,
};

use crate::web::frames::DecodedVideoFrames;
use crate::web::timestamp::timestamp_us_to_i32;

fn take_js_err(cell: &RefCell<Option<JsValue>>) -> Result<(), JsValue> {
    cell.borrow_mut().take().map_or(Ok(()), Err)
}

fn video_decoder_config(
    codec: &str,
    width: u32,
    height: u32,
    description: Option<&[u8]>,
) -> WebVideoDecoderConfig {
    let cfg = WebVideoDecoderConfig::new(codec);
    cfg.set_coded_width(width);
    cfg.set_coded_height(height);
    if let Some(desc) = description {
        cfg.set_description_u8_array(&Uint8Array::from(desc));
    }
    cfg
}

fn timestamp_i32(timestamp_us: f64) -> Result<i32, JsValue> {
    timestamp_us_to_i32(timestamp_us)
        .ok_or_else(|| JsValue::from_str("timestamp does not fit i32 microseconds"))
}

/// Returns whether `WebCodecs` can decode `codec` at `width`x`height` in this browser.
///
/// Uses `VideoDecoder.isConfigSupported`; codec-parameterized so callers can probe H.264,
/// VP8, VP9, AV1, ... — see `docs/ai/wiki/decode/web-video-decode.md` for which codecs this
/// crate's Chromium test build actually supports (H.264 decode is known-unsupported there;
/// VP8/VP9/AV1 are supported).
#[cfg(feature = "video")]
#[wasm_bindgen]
pub async fn is_webcodecs_video_decode_supported(codec: String, width: u32, height: u32) -> bool {
    let cfg = video_decoder_config(&codec, width, height, None);
    // `is_config_supported` resolves to a `VideoDecoderSupport` dictionary (`{supported,
    // config}`) — `JsFuture`'s typed `Promise<T>` support (js-sys 0.3.103+) already yields
    // that dictionary directly (no further `dyn_into` cast needed; casting a
    // WebIDL-dictionary-shaped extern type to itself is unreliable since it has no real JS
    // constructor to `instanceof`-check against).
    JsFuture::from(VideoDecoder::is_config_supported(&cfg))
        .await
        .ok()
        .and_then(|support| support.get_supported())
        .unwrap_or(false)
}

/// Decode a run of `EncodedVideoChunk`s and read each output `VideoFrame`'s luma plane back
/// to a CPU buffer via `VideoFrame::copyTo`.
///
/// Chunk payloads are passed flattened (`chunk_data` plus parallel `chunk_offsets` /
/// `chunk_lengths`) rather than as a `Vec<Vec<u8>>` or a `Vec` of `web-sys` objects:
/// wasm-bindgen values are scoped to a single wasm module/instance, so results from
/// `mediaway-encoder-web`'s `encode_video_frames` (compiled to a *separate* wasm module) can
/// only cross into this crate as plain JS arrays/typed arrays, never as shared Rust types.
/// See `tools/e2e-web/tests/decode-trim-splice.spec.ts` for the JS-side flattening.
///
/// `description`, when present, is set verbatim as `VideoDecoderConfig.description` (e.g. an
/// `avcC` record for H.264); the codecs this crate's test build actually decodes (VP8/VP9)
/// need none.
#[cfg(feature = "video")]
#[wasm_bindgen]
#[allow(
    clippy::too_many_arguments,
    reason = "flattened chunk list crossing a wasm module boundary; see doc comment"
)]
pub async fn decode_video_chunks(
    codec: String,
    width: u32,
    height: u32,
    description: Option<Vec<u8>>,
    chunk_data: Vec<u8>,
    chunk_offsets: Vec<u32>,
    chunk_lengths: Vec<u32>,
    chunk_timestamps_us: Vec<f64>,
    chunk_is_key: Vec<u8>,
) -> Result<DecodedVideoFrames, JsValue> {
    let n = chunk_offsets.len();
    if chunk_lengths.len() != n || chunk_timestamps_us.len() != n || chunk_is_key.len() != n {
        return Err(JsValue::from_str("chunk array length mismatch"));
    }

    let frames: Rc<RefCell<Vec<VideoFrame>>> = Rc::new(RefCell::new(Vec::new()));
    // clone: Rc callback share
    let frames_cb = frames.clone();
    let output = Closure::wrap(Box::new(move |frame: VideoFrame| {
        frames_cb.borrow_mut().push(frame);
    }) as Box<dyn FnMut(VideoFrame)>);
    let dec_err: Rc<RefCell<Option<JsValue>>> = Rc::new(RefCell::new(None));
    // clone: Rc callback share
    let dec_err_cb = dec_err.clone();
    let error = Closure::wrap(Box::new(move |e: JsValue| {
        *dec_err_cb.borrow_mut() = Some(e);
    }) as Box<dyn FnMut(JsValue)>);
    let init = VideoDecoderInit::new(
        error.as_ref().unchecked_ref(),
        output.as_ref().unchecked_ref(),
    );
    let dec = VideoDecoder::new(&init).map_err(|_| JsValue::from_str("VideoDecoder::new"))?;
    error.forget();
    output.forget();

    let cfg = video_decoder_config(&codec, width, height, description.as_deref());
    dec.configure(&cfg)
        .map_err(|_| JsValue::from_str("VideoDecoder::configure"))?;

    #[allow(
        clippy::needless_range_loop,
        reason = "indexes four parallel input vecs plus a chunk_data slice by position"
    )]
    for i in 0..n {
        let start = chunk_offsets[i] as usize;
        let end = start + chunk_lengths[i] as usize;
        let payload = chunk_data
            .get(start..end)
            .ok_or_else(|| JsValue::from_str("chunk offset/length out of bounds"))?;
        let kind = if chunk_is_key[i] == 0 {
            EncodedVideoChunkType::Delta
        } else {
            EncodedVideoChunkType::Key
        };
        let timestamp = timestamp_i32(chunk_timestamps_us[i])?;
        let chunk_init =
            EncodedVideoChunkInit::new_with_u8_array(&Uint8Array::from(payload), timestamp, kind);
        let chunk = EncodedVideoChunk::new(&chunk_init)
            .map_err(|_| JsValue::from_str("EncodedVideoChunk::new"))?;
        dec.decode(&chunk)
            .map_err(|_| JsValue::from_str("VideoDecoder::decode"))?;
    }
    JsFuture::from(dec.flush()).await?;
    // Release the (possibly hardware-backed) decode session promptly — see
    // `mediaway-encoder-web`'s matching `enc.close()` fix in `docs/ai/wiki/encode/web-gpu-frame.md`.
    let _ = dec.close();
    take_js_err(&dec_err)?;

    // Drain into an owned `Vec` first — holding the `RefCell` borrow across the `.await`
    // below (inside the loop) would be fragile even though nothing else touches `frames`
    // today.
    let decoded_frames: Vec<VideoFrame> = frames.borrow_mut().drain(..).collect();
    let mut timestamps_us = Vec::new();
    let mut luma_planes = Vec::new();
    for frame in decoded_frames {
        timestamps_us.push(frame.timestamp());
        luma_planes.push(read_luma_plane(&frame, width, height).await?);
        frame.close();
    }
    Ok(DecodedVideoFrames::new(timestamps_us, luma_planes))
}

/// Read back `frame`'s luma (Y) plane to a tightly packed `width * height` CPU buffer via
/// `VideoFrame::copyTo`, using the returned `PlaneLayout` (plane 0) to de-stride rows —
/// `codedWidth` may exceed `width` (macroblock/superblock padding), so the raw copy can have
/// row stride greater than `width`.
#[cfg(feature = "video")]
async fn read_luma_plane(frame: &VideoFrame, width: u32, height: u32) -> Result<Vec<u8>, JsValue> {
    let size = frame
        .allocation_size()
        .map_err(|_| JsValue::from_str("VideoFrame::allocationSize"))?;
    let readback = Uint8Array::new_with_length(size);
    // `copyTo` resolves to a typed `Array<PlaneLayout>` (js-sys 0.3.103+'s typed
    // `Promise`/`Array` support already yield the `PlaneLayout` values directly — no
    // `dyn_into` cast needed, which is unreliable for WebIDL-dictionary-shaped types like
    // `PlaneLayout` that have no real JS constructor to `instanceof`-check against).
    let layouts = JsFuture::from(frame.copy_to_with_u8_array(&readback))
        .await
        .map_err(|_| JsValue::from_str("VideoFrame::copyTo"))?;
    let luma_layout: PlaneLayout = layouts.get(0);
    let stride = luma_layout.get_stride() as usize;
    let offset = luma_layout.get_offset() as usize;
    let raw = readback.to_vec();
    let (width, height) = (width as usize, height as usize);
    let mut luma = vec![0u8; width * height];
    for row in 0..height {
        let row_start = offset + row * stride;
        let out_start = row * width;
        luma[out_start..out_start + width].copy_from_slice(
            raw.get(row_start..row_start + width)
                .ok_or_else(|| JsValue::from_str("copyTo buffer too small for stride"))?,
        );
    }
    Ok(luma)
}
