//! `WebCodecs` decode backend — wasm32 browser implementation.

#![forbid(unsafe_code)]

use std::cell::RefCell;
use std::rc::Rc;

use js_sys::Uint8Array;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AudioData, AudioDataCopyToOptions, AudioDecoder, AudioDecoderConfig as WebAudioDecoderConfig,
    AudioDecoderInit, AudioSampleFormat, EncodedAudioChunk, EncodedAudioChunkInit,
    EncodedAudioChunkType, EncodedVideoChunk, EncodedVideoChunkInit, EncodedVideoChunkType,
    PlaneLayout, VideoDecoder, VideoDecoderConfig as WebVideoDecoderConfig, VideoDecoderInit,
    VideoFrame,
};

use crate::web::audio_frames::DecodedAudioData;
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

fn audio_decoder_config(
    codec: &str,
    channels: u32,
    sample_rate: u32,
    description: Option<&[u8]>,
) -> WebAudioDecoderConfig {
    let cfg = WebAudioDecoderConfig::new(codec, channels, sample_rate);
    if let Some(desc) = description {
        cfg.set_description_u8_array(&Uint8Array::from(desc));
    }
    cfg
}

/// Returns whether `WebCodecs` can decode `codec` at `channels`/`sample_rate` in this browser.
///
/// First audio decode probe in this module — mirrors [`is_webcodecs_video_decode_supported`];
/// see `crates/mediaway-decoder/adr/web/0001…` for why `mediaway-decoder::web` had no audio
/// surface before this.
#[cfg(feature = "audio")]
#[wasm_bindgen]
pub async fn is_webcodecs_audio_decode_supported(
    codec: String,
    channels: u32,
    sample_rate: u32,
) -> bool {
    let cfg = audio_decoder_config(&codec, channels, sample_rate, None);
    // Same `{supported, config}` dictionary shape as `is_webcodecs_video_decode_supported` —
    // see its comment above.
    JsFuture::from(AudioDecoder::is_config_supported(&cfg))
        .await
        .ok()
        .and_then(|support| support.get_supported())
        .unwrap_or(false)
}

/// Decode a run of `EncodedAudioChunk`s and read each output `AudioData`'s samples back to a
/// channel-interleaved `f32` CPU buffer.
///
/// Chunk payloads are passed flattened (`chunk_data` plus parallel `chunk_offsets` /
/// `chunk_lengths` / `chunk_timestamps_us`), the same wasm-module-boundary-crossing shape
/// [`decode_video_chunks`] uses — see its doc comment. Unlike the video version, there is no
/// `chunk_is_key` parameter: every constructed `EncodedAudioChunk` uses
/// `EncodedAudioChunkType::Key`, since Opus/AAC packets are independently decodable
/// per-packet in practice (`EncodedAudioChunkType::Delta` is unused here; revisit only if a
/// real codec needs it).
///
/// `description`, when present, is set verbatim as `AudioDecoderConfig.description` (e.g. an
/// AAC `AudioSpecificConfig`); Opus normally needs none for a bare `EncodedAudioChunk`-level
/// round trip (no `OpusHead` container box in this crate's smoke path — see the sibling
/// encoder-side ADR's `iso-bmff` caveat).
///
/// # Sample readback shape
///
/// Trusts the browser's own reported `AudioData.format()` rather than forcing a resample/
/// format conversion (no `AudioDataCopyToOptions.format` override) — the same "de-stride
/// using the browser's own reported layout" posture [`read_luma_plane`] uses for
/// `VideoFrame`. `WebCodecs`' `AudioData.copyTo` takes a `planeIndex` and copies one plane per
/// call, so a genuinely planar format (`u8-planar`/`s16-planar`/`s32-planar`/`f32-planar`)
/// needs one `copyTo` call per channel; an interleaved format (`u8`/`s16`/`s32`/`f32`) needs
/// exactly one. Either way the raw bytes are converted to `f32` per [`decode_audio_samples`]
/// and interleaved into a single flat buffer so callers get one consistent shape regardless
/// of the browser's native layout. **This exact byte layout is unverified against a real
/// browser in this environment** (wasm32 compile-verified only, no browser runtime here) —
/// see Open Questions in the sibling ADR.
#[cfg(feature = "audio")]
#[wasm_bindgen]
#[allow(
    clippy::too_many_arguments,
    reason = "flattened chunk list crossing a wasm module boundary; see doc comment"
)]
pub async fn decode_audio_chunks(
    codec: String,
    channels: u32,
    sample_rate: u32,
    description: Option<Vec<u8>>,
    chunk_data: Vec<u8>,
    chunk_offsets: Vec<u32>,
    chunk_lengths: Vec<u32>,
    chunk_timestamps_us: Vec<f64>,
) -> Result<DecodedAudioData, JsValue> {
    let n = chunk_offsets.len();
    if chunk_lengths.len() != n || chunk_timestamps_us.len() != n {
        return Err(JsValue::from_str("chunk array length mismatch"));
    }

    let frames: Rc<RefCell<Vec<AudioData>>> = Rc::new(RefCell::new(Vec::new()));
    // clone: Rc callback share
    let frames_cb = frames.clone();
    let output = Closure::wrap(Box::new(move |frame: AudioData| {
        frames_cb.borrow_mut().push(frame);
    }) as Box<dyn FnMut(AudioData)>);
    let dec_err: Rc<RefCell<Option<JsValue>>> = Rc::new(RefCell::new(None));
    // clone: Rc callback share
    let dec_err_cb = dec_err.clone();
    let error = Closure::wrap(Box::new(move |e: JsValue| {
        *dec_err_cb.borrow_mut() = Some(e);
    }) as Box<dyn FnMut(JsValue)>);
    let init = AudioDecoderInit::new(
        error.as_ref().unchecked_ref(),
        output.as_ref().unchecked_ref(),
    );
    let dec = AudioDecoder::new(&init).map_err(|_| JsValue::from_str("AudioDecoder::new"))?;
    error.forget();
    output.forget();

    let cfg = audio_decoder_config(&codec, channels, sample_rate, description.as_deref());
    dec.configure(&cfg)
        .map_err(|_| JsValue::from_str("AudioDecoder::configure"))?;

    #[allow(
        clippy::needless_range_loop,
        reason = "indexes three parallel input vecs plus a chunk_data slice by position"
    )]
    for i in 0..n {
        let start = chunk_offsets[i] as usize;
        let end = start + chunk_lengths[i] as usize;
        let payload = chunk_data
            .get(start..end)
            .ok_or_else(|| JsValue::from_str("chunk offset/length out of bounds"))?;
        let timestamp = timestamp_i32(chunk_timestamps_us[i])?;
        let chunk_init = EncodedAudioChunkInit::new_with_u8_array(
            &Uint8Array::from(payload),
            timestamp,
            EncodedAudioChunkType::Key,
        );
        let chunk = EncodedAudioChunk::new(&chunk_init)
            .map_err(|_| JsValue::from_str("EncodedAudioChunk::new"))?;
        dec.decode(&chunk)
            .map_err(|_| JsValue::from_str("AudioDecoder::decode"))?;
    }
    JsFuture::from(dec.flush()).await?;
    // Release the (possibly hardware-backed) decode session promptly — same hygiene fix as
    // `decode_video_chunks`'s `dec.close()`.
    let _ = dec.close();
    take_js_err(&dec_err)?;

    // Drain into an owned `Vec` first, same reasoning as `decode_video_chunks` — no `.await`
    // needed here (unlike `VideoFrame::copyTo`, `AudioData::copyTo` is synchronous), but keeps
    // the two decode paths structurally symmetric.
    let decoded_frames: Vec<AudioData> = frames.borrow_mut().drain(..).collect();
    let mut timestamps_us = Vec::with_capacity(decoded_frames.len());
    let mut sample_counts = Vec::with_capacity(decoded_frames.len());
    let mut channel_counts = Vec::with_capacity(decoded_frames.len());
    let mut samples = Vec::with_capacity(decoded_frames.len());
    for frame in decoded_frames {
        timestamps_us.push(frame.timestamp());
        let (channel_count, sample_count, data) = decode_audio_samples(&frame)?;
        channel_counts.push(channel_count);
        sample_counts.push(sample_count);
        samples.push(data);
        frame.close();
    }
    Ok(DecodedAudioData::new(
        timestamps_us,
        sample_counts,
        channel_counts,
        samples,
    ))
}

/// Read back `frame`'s samples to a channel-interleaved `f32` buffer, trusting the browser's
/// own reported `AudioData.format()` (see [`decode_audio_chunks`]'s doc comment for the
/// planar-vs-interleaved posture). Returns `(channel_count, sample_count, interleaved_samples)`
/// where `sample_count` is `AudioData.numberOfFrames` (samples per channel).
#[cfg(feature = "audio")]
fn decode_audio_samples(frame: &AudioData) -> Result<(u32, u32, Vec<f32>), JsValue> {
    let format = frame
        .format()
        .ok_or_else(|| JsValue::from_str("AudioData::format missing"))?;
    let channels = frame.number_of_channels();
    let sample_count = frame.number_of_frames();
    let planar = matches!(
        format,
        AudioSampleFormat::U8Planar
            | AudioSampleFormat::S16Planar
            | AudioSampleFormat::S32Planar
            | AudioSampleFormat::F32Planar
    );

    if planar {
        let mut interleaved = vec![0.0f32; (sample_count * channels) as usize];
        for channel in 0..channels {
            let options = AudioDataCopyToOptions::new(channel);
            let size = frame
                .allocation_size(&options)
                .map_err(|_| JsValue::from_str("AudioData::allocationSize"))?;
            let mut buf = vec![0u8; size as usize];
            frame
                .copy_to_with_u8_slice(&mut buf, &options)
                .map_err(|_| JsValue::from_str("AudioData::copyTo"))?;
            for (i, sample) in pcm_bytes_to_f32(&buf, format)?.into_iter().enumerate() {
                interleaved[i * channels as usize + channel as usize] = sample;
            }
        }
        Ok((channels, sample_count, interleaved))
    } else {
        let options = AudioDataCopyToOptions::new(0);
        let size = frame
            .allocation_size(&options)
            .map_err(|_| JsValue::from_str("AudioData::allocationSize"))?;
        let mut buf = vec![0u8; size as usize];
        frame
            .copy_to_with_u8_slice(&mut buf, &options)
            .map_err(|_| JsValue::from_str("AudioData::copyTo"))?;
        Ok((channels, sample_count, pcm_bytes_to_f32(&buf, format)?))
    }
}

/// Convert raw PCM bytes in `format` to `f32` samples, normalized to `[-1.0, 1.0]` for the
/// integer formats (`f32`/`f32-planar` pass through unchanged). Little-endian, matching
/// `WebCodecs`' `AudioData` byte layout.
///
/// `AudioSampleFormat` is `#[non_exhaustive]` in `web-sys` (mirrors the spec potentially
/// growing new formats) — an unrecognized format is reported as an error rather than a panic
/// (`unwrap`/`panic!` are denied outside tests in this crate), not silently ignored or
/// guessed at.
#[cfg(feature = "audio")]
#[allow(
    clippy::cast_precision_loss,
    reason = "sample values are small integers (u8/i16/i32 range), precision loss is the intended [-1.0, 1.0] normalization"
)]
fn pcm_bytes_to_f32(raw: &[u8], format: AudioSampleFormat) -> Result<Vec<f32>, JsValue> {
    match format {
        AudioSampleFormat::U8 | AudioSampleFormat::U8Planar => Ok(raw
            .iter()
            .map(|&b| (f32::from(b) - 128.0) / 128.0)
            .collect()),
        AudioSampleFormat::S16 | AudioSampleFormat::S16Planar => Ok(raw
            .chunks_exact(2)
            .map(|c| f32::from(i16::from_le_bytes([c[0], c[1]])) / f32::from(i16::MAX))
            .collect()),
        AudioSampleFormat::S32 | AudioSampleFormat::S32Planar => Ok(raw
            .chunks_exact(4)
            .map(|c| (i32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f32) / (i32::MAX as f32))
            .collect()),
        AudioSampleFormat::F32 | AudioSampleFormat::F32Planar => Ok(raw
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect()),
        _ => Err(JsValue::from_str("unrecognized AudioSampleFormat")),
    }
}
