//! WebCodecs encode backend — wasm32 browser implementation.

#![forbid(unsafe_code)]

use std::cell::RefCell;
use std::rc::Rc;

use bytes::Bytes;
use iso_bmff::{Codec, Demuxer, Error, Muxer, Rational, Sample, Track};
use js_sys::Float32Array;
use js_sys::Uint8Array;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AudioData, AudioDataInit, AudioEncoder, AudioEncoderConfig as WebAudioEncoderConfig,
    AudioEncoderInit, AudioSampleFormat, EncodedAudioChunk, EncodedVideoChunk,
    EncodedVideoChunkType, Gpu, GpuCanvasAlphaMode, GpuCanvasConfiguration, GpuCanvasContext,
    GpuColorDict, GpuDevice, GpuLoadOp, GpuRenderPassColorAttachment, GpuRenderPassDescriptor,
    GpuStoreOp, GpuTexture, OffscreenCanvas, VideoEncoder,
    VideoEncoderConfig as WebVideoEncoderConfig, VideoEncoderInit, VideoFrame,
    VideoFrameBufferInit, VideoFrameInit, VideoPixelFormat,
};

use crate::web::chunks::EncodedVideoChunks;
use crate::web::config::{WebAudioOpenConfig, WebVideoOpenConfig};
use crate::web::timestamp::timestamp_us_to_i32;

/// Returns whether WebCodecs H.264 + AAC configs are supported in this browser.
#[cfg(all(feature = "audio", feature = "video"))]
#[wasm_bindgen]
pub async fn is_webcodecs_av_supported() -> bool {
    video_supported().await && audio_supported().await
}

#[cfg(feature = "video")]
async fn video_codec_supported(codec: &str) -> bool {
    let cfg = WebVideoEncoderConfig::new(codec, 64, 64);
    cfg.set_bitrate(500_000);
    let promise = VideoEncoder::is_config_supported(&cfg);
    // `is_config_supported` resolves to a `VideoEncoderSupport` dictionary (`{supported,
    // config}`), not a boolean — `JsFuture`'s typed `Promise<T>` support (js-sys 0.3.103+)
    // already yields that dictionary directly, so `get_supported()` reads the field with no
    // extra cast. (An earlier `.as_bool()` on this same value always returned `None`/`false`
    // regardless of real browser support — a latent bug this fixes.)
    let reported = JsFuture::from(promise)
        .await
        .ok()
        .and_then(|v| v.get_supported())
        .unwrap_or(false);
    if !reported {
        return false;
    }
    // `isConfigSupported` can still report `true` for a codec whose actual encoder isn't
    // wired up in this browser build — observed on this Chromium: `avc1.42E01E` reports
    // supported, then a real `configure()`/`encode()`/`flush()` throws `OperationError:
    // Encoding error` (no bundled H.264 software encoder, presumably). Confirm with one real
    // tiny encode rather than trusting the capability query alone.
    let Ok(frame) = black_nv12_frame(64, 64) else {
        return false;
    };
    let ok = encode_frame_via(&frame, codec).await.is_ok();
    frame.close();
    ok
}

#[cfg(feature = "video")]
async fn video_supported() -> bool {
    video_codec_supported("avc1.42E01E").await
}

/// Probe WebCodecs support for a video codec string (`avc1…`, `hev1…`, `av01…`, `vp09…`).
#[cfg(feature = "video")]
#[wasm_bindgen]
pub async fn is_webcodecs_video_codec_supported(codec: String) -> bool {
    video_codec_supported(&codec).await
}

#[cfg(feature = "audio")]
async fn audio_supported() -> bool {
    // `AudioEncoderConfig::new`'s web-sys signature is `(codec, number_of_channels,
    // sample_rate)` — NOT `(codec, sample_rate, number_of_channels)`. Swapped args here used
    // to build a nonsensical config (48,000 channels at 2 Hz), which `isConfigSupported`
    // correctly rejected — the real, deterministic reason `is_webcodecs_av_supported` always
    // reported unsupported, not the `VideoEncoder`/`AudioEncoder` `.close()` resource-hygiene
    // issue fixed alongside this.
    let cfg = WebAudioEncoderConfig::new("mp4a.40.2", 2, 48_000);
    let promise = AudioEncoder::is_config_supported(&cfg);
    // Same `{supported, config}` dictionary shape as `video_codec_supported` — see its
    // comment above.
    JsFuture::from(promise)
        .await
        .ok()
        .and_then(|v| v.get_supported())
        .unwrap_or(false)
}

/// Encode one black NV12 frame + short silence via WebCodecs, mux to fMP4, return bytes.
#[cfg(all(feature = "audio", feature = "video"))]
#[wasm_bindgen]
pub async fn webcodecs_av_fmp4_smoke() -> Result<Vec<u8>, JsValue> {
    if !is_webcodecs_av_supported().await {
        return Err(JsValue::from_str("WebCodecs H.264/AAC not supported"));
    }
    let vchunk = encode_one_h264_frame().await?;
    let achunk = encode_one_aac_buffer().await?;
    mux_av_chunks(&vchunk, &achunk)
}

/// Probe whether this browser can source a WebCodecs `VideoFrame` from a WebGPU-backed
/// canvas: WebCodecs H.264 support, plus `navigator.gpu` and a grantable adapter/device.
#[cfg(feature = "video")]
#[wasm_bindgen]
pub async fn is_webgpu_video_frame_supported() -> bool {
    video_supported().await && request_gpu_device().await.is_ok()
}

/// Encode one WebGPU-resident frame via WebCodecs, mux to fMP4, return bytes. Video-only
/// companion to [`webcodecs_av_fmp4_smoke`] exercising the Stage 2 (Web) roadmap item
/// "`GPUTexture` → encode Zero-Copy" — see [`webgpu_canvas_frame`] for the honest cost
/// contract (GPU-resident, no CPU readback in the Mediaway path; not an unconditional
/// Zero-Copy guarantee, since a raw `GPUTexture` cannot be passed to `VideoFrame` directly).
#[cfg(feature = "video")]
#[wasm_bindgen]
pub async fn webcodecs_gpu_video_fmp4_smoke() -> Result<Vec<u8>, JsValue> {
    if !is_webgpu_video_frame_supported().await {
        return Err(JsValue::from_str(
            "WebCodecs H.264 or WebGPU device not supported",
        ));
    }
    let vchunk = encode_one_h264_frame_from_webgpu_canvas().await?;
    mux_video_chunk(&vchunk)
}

fn iso_err(error: &Error) -> JsValue {
    JsValue::from_str(&error.to_string())
}

fn take_js_err(cell: &RefCell<Option<JsValue>>) -> Result<(), JsValue> {
    cell.borrow_mut().take().map_or(Ok(()), Err)
}

#[cfg(feature = "video")]
async fn encode_one_h264_frame() -> Result<Vec<u8>, JsValue> {
    let frame = black_nv12_frame(64, 64)?;
    encode_frame_via(&frame, "avc1.42E01E").await
}

/// Configure a `VideoEncoder` for `codec`, encode one `frame`, flush, and drain the first
/// chunk.
///
/// Shared by the CPU NV12 path ([`encode_one_h264_frame`]), the WebGPU-canvas path
/// ([`encode_one_h264_frame_from_webgpu_canvas`]), and [`video_codec_supported`]'s real-encode
/// probe — all three just need *some* `VideoFrame` encoded through WebCodecs.
#[cfg(feature = "video")]
async fn encode_frame_via(frame: &VideoFrame, codec: &str) -> Result<Vec<u8>, JsValue> {
    let chunks: Rc<RefCell<Vec<Vec<u8>>>> = Rc::new(RefCell::new(Vec::new()));
    // clone: Rc callback share
    let chunks_cb = chunks.clone();
    let copy_err: Rc<RefCell<Option<JsValue>>> = Rc::new(RefCell::new(None));
    // clone: Rc callback share
    let copy_err_cb = copy_err.clone();
    let output = Closure::wrap(Box::new(move |chunk: EncodedVideoChunk| {
        let mut buf = vec![0u8; chunk.byte_length() as usize];
        if chunk.copy_to_with_u8_slice(&mut buf).is_err() {
            *copy_err_cb.borrow_mut() =
                Some(JsValue::from_str("EncodedVideoChunk::copy_to failed"));
            return;
        }
        chunks_cb.borrow_mut().push(buf);
    }) as Box<dyn FnMut(EncodedVideoChunk)>);
    let enc_err: Rc<RefCell<Option<JsValue>>> = Rc::new(RefCell::new(None));
    // clone: Rc callback share
    let enc_err_cb = enc_err.clone();
    let error = Closure::wrap(Box::new(move |e: JsValue| {
        *enc_err_cb.borrow_mut() = Some(e);
    }) as Box<dyn FnMut(JsValue)>);
    let init = VideoEncoderInit::new(
        error.as_ref().unchecked_ref(),
        output.as_ref().unchecked_ref(),
    );
    let enc = VideoEncoder::new(&init).map_err(|_| JsValue::from_str("VideoEncoder::new"))?;
    error.forget();
    output.forget();

    let cfg = WebVideoEncoderConfig::new(codec, 64, 64);
    cfg.set_width(64);
    cfg.set_height(64);
    cfg.set_bitrate(500_000);
    cfg.set_framerate(30.0);
    enc.configure(&cfg)
        .map_err(|_| JsValue::from_str("VideoEncoder::configure"))?;

    enc.encode(frame)
        .map_err(|_| JsValue::from_str("VideoEncoder::encode"))?;
    JsFuture::from(enc.flush()).await?;
    // Release the (possibly hardware-backed) encode session promptly: leaving it open let
    // enough concurrent probes in one page exhaust the real encoder's session pool, making
    // `video_codec_supported`'s real-encode check spuriously fail on a later call in the same
    // page — see `docs/ai/wiki/encode/web-gpu-frame.md`.
    let _ = enc.close();
    take_js_err(&enc_err)?;
    take_js_err(&copy_err)?;

    chunks
        .borrow()
        .first()
        .cloned() // clone: take owned chunk buffer out of RefCell
        .ok_or_else(|| JsValue::from_str("no video chunk"))
}

/// Get `navigator.gpu`, erroring out on hosts without a `Window` (e.g. workers — not
/// exercised by this crate today).
#[cfg(feature = "video")]
fn navigator_gpu() -> Result<Gpu, JsValue> {
    Ok(web_sys::window()
        .ok_or_else(|| JsValue::from_str("no window"))?
        .navigator()
        .gpu())
}

/// Request a WebGPU adapter + device. `Err` when WebGPU is absent or no adapter is granted
/// (software-only hosts, permissions policy, disabled flag, …).
#[cfg(feature = "video")]
async fn request_gpu_device() -> Result<GpuDevice, JsValue> {
    let gpu = navigator_gpu()?;
    let adapter = JsFuture::from(gpu.request_adapter())
        .await?
        .into_option()
        .ok_or_else(|| JsValue::from_str("no WebGPU adapter"))?;
    JsFuture::from(adapter.request_device()).await
}

/// Build a `VideoFrame` sourced from a WebGPU-backed `OffscreenCanvas` — no CPU pixel
/// buffer anywhere in this function.
///
/// # Why not a bare `GPUTexture`?
///
/// WebCodecs' `VideoFrame` constructor only accepts a `CanvasImageSource`
/// (`HTMLCanvasElement` / `OffscreenCanvas` / `ImageBitmap` / `HTMLVideoElement` / …,
/// [WebCodecs §5.1](https://w3c.github.io/webcodecs/#dom-videoframe-videoframe)); a raw
/// `GPUTexture` is not a member of that union, and this crate's `web-sys` 0.3.103 bindings
/// (generated from the same IDL) expose no `VideoFrame` constructor that takes one. Verified
/// empirically on Chromium 148 (headless, this machine's Playwright build): `new
/// VideoFrame(texture, { timestamp: 0 })` throws `TypeError: Overload resolution failed`,
/// while `new VideoFrame(canvas, { timestamp: 0 })` succeeds. So the supported GPU-resident
/// path renders/writes into an `OffscreenCanvas` configured with a `"webgpu"`
/// (`GPUCanvasContext`) context, then builds the `VideoFrame` from that **canvas** — this
/// function writes a solid clear color straight into the canvas's current WebGPU texture via
/// a one-attachment render pass (`GPULoadOp::Clear`), never reading pixels back to the CPU.
///
/// # Zero-Copy honesty
///
/// This function and its caller never allocate a CPU pixel `Vec`, never call
/// `VideoFrame::copyTo`/`allocationSize`, and never map/read a `GPUBuffer` — the payload
/// stays GPU-resident on the Mediaway side end to end. Whether the browser's internal
/// `VideoFrame` representation *shares* the canvas's compositor texture or performs its own
/// internal GPU→GPU copy is implementation-defined by the WebCodecs/WebGPU specs and is not
/// observable from JS/wasm (no timing/inspection API exposes it). This path is therefore
/// documented as **GPU-resident, no CPU readback in the Mediaway path** — not as an
/// unconditional Zero-Copy guarantee. See `docs/spec/caveats-and-clarity.md` § Catalog
/// (`webgpu_canvas_frame` row) for the same caveat in the workspace catalog.
#[cfg(feature = "video")]
async fn webgpu_canvas_frame(width: u32, height: u32) -> Result<VideoFrame, JsValue> {
    let gpu = navigator_gpu()?;
    let device = request_gpu_device().await?;
    let format = gpu.get_preferred_canvas_format();

    let canvas = OffscreenCanvas::new(width, height)?;
    let ctx_obj = canvas
        .get_context("webgpu")?
        .ok_or_else(|| JsValue::from_str("no webgpu canvas context"))?;
    let ctx: GpuCanvasContext = ctx_obj
        .dyn_into()
        .map_err(|_| JsValue::from_str("canvas context is not a GPUCanvasContext"))?;

    let canvas_config = GpuCanvasConfiguration::new(&device, format);
    canvas_config.set_alpha_mode(GpuCanvasAlphaMode::Opaque);
    ctx.configure(&canvas_config)?;

    // GPU-resident write: clears the canvas's current texture in place via a render
    // pass. No `Vec<u8>` / CPU staging buffer is allocated anywhere in this function.
    let texture: GpuTexture = ctx.get_current_texture()?;
    let view = texture.create_view()?;
    let attachment = GpuRenderPassColorAttachment::new_with_gpu_texture_view(
        GpuLoadOp::Clear,
        GpuStoreOp::Store,
        &view,
    );
    attachment.set_clear_value_gpu_color_dict(&GpuColorDict::new(1.0, 0.0, 0.0, 1.0));
    let pass_desc = GpuRenderPassDescriptor::new(&[js_sys::JsOption::wrap(attachment)]);
    let encoder = device.create_command_encoder();
    let pass = encoder.begin_render_pass(&pass_desc)?;
    pass.end();
    device.queue().submit(&[encoder.finish()]);

    let frame_init = VideoFrameInit::new();
    frame_init.set_timestamp(0);
    VideoFrame::new_with_offscreen_canvas_and_video_frame_init(&canvas, &frame_init)
        .map_err(|_| JsValue::from_str("VideoFrame::new from WebGPU canvas failed"))
}

#[cfg(feature = "video")]
async fn encode_one_h264_frame_from_webgpu_canvas() -> Result<Vec<u8>, JsValue> {
    let frame = webgpu_canvas_frame(64, 64).await?;
    let result = encode_frame_via(&frame, "avc1.42E01E").await;
    frame.close();
    result
}

#[cfg(feature = "audio")]
async fn encode_one_aac_buffer() -> Result<Vec<u8>, JsValue> {
    let chunks: Rc<RefCell<Vec<Vec<u8>>>> = Rc::new(RefCell::new(Vec::new()));
    // clone: Rc callback share
    let chunks_cb = chunks.clone();
    let copy_err: Rc<RefCell<Option<JsValue>>> = Rc::new(RefCell::new(None));
    // clone: Rc callback share
    let copy_err_cb = copy_err.clone();
    let output = Closure::wrap(Box::new(move |chunk: EncodedAudioChunk| {
        let mut buf = vec![0u8; chunk.byte_length() as usize];
        if chunk.copy_to_with_u8_slice(&mut buf).is_err() {
            *copy_err_cb.borrow_mut() =
                Some(JsValue::from_str("EncodedAudioChunk::copy_to failed"));
            return;
        }
        chunks_cb.borrow_mut().push(buf);
    }) as Box<dyn FnMut(EncodedAudioChunk)>);
    let enc_err: Rc<RefCell<Option<JsValue>>> = Rc::new(RefCell::new(None));
    // clone: Rc callback share
    let enc_err_cb = enc_err.clone();
    let error = Closure::wrap(Box::new(move |e: JsValue| {
        *enc_err_cb.borrow_mut() = Some(e);
    }) as Box<dyn FnMut(JsValue)>);
    let init = AudioEncoderInit::new(
        error.as_ref().unchecked_ref(),
        output.as_ref().unchecked_ref(),
    );
    let enc = AudioEncoder::new(&init).map_err(|_| JsValue::from_str("AudioEncoder::new"))?;
    error.forget();
    output.forget();

    let cfg = WebAudioEncoderConfig::new("mp4a.40.2", 2, 48_000); // (codec, channels, sample_rate)
    cfg.set_bitrate(128_000);
    enc.configure(&cfg)
        .map_err(|_| JsValue::from_str("AudioEncoder::configure"))?;

    // A real (non-simulated) AAC encoder needs more than one 1024-sample AAC frame buffered
    // before it can flush a complete output chunk (MDCT look-ahead/priming delay) — a single
    // 1024-frame `AudioData` reliably throws `EncodingError: Flushing error` on real Chrome;
    // verified empirically that >=2048 frames (2 AAC frames' worth) flushes cleanly, so this
    // uses a safety margin of 4 frames.
    const FRAME_COUNT: u32 = 4096;
    let data = silence_f32_interleaved(2, FRAME_COUNT);
    let arr = Float32Array::new_with_length(data.len() as u32);
    for (i, sample) in data.iter().enumerate() {
        arr.set_index(i as u32, *sample);
    }
    let audio_init = AudioDataInit::new(
        arr.as_ref(),
        AudioSampleFormat::F32,
        2,
        FRAME_COUNT,
        48_000.0,
        0,
    );
    let audio = AudioData::new(&audio_init).map_err(|_| JsValue::from_str("AudioData::new"))?;
    enc.encode(&audio)
        .map_err(|_| JsValue::from_str("AudioEncoder::encode"))?;
    audio.close();
    JsFuture::from(enc.flush()).await?;
    let _ = enc.close(); // see encode_frame_via's close() comment — same hygiene fix
    take_js_err(&enc_err)?;
    take_js_err(&copy_err)?;

    chunks
        .borrow()
        .first()
        .cloned() // clone: take owned chunk buffer out of RefCell
        .ok_or_else(|| JsValue::from_str("no audio chunk"))
}

#[cfg(feature = "video")]
fn black_nv12_frame(width: u32, height: u32) -> Result<VideoFrame, JsValue> {
    let y = (width * height) as usize;
    let uv = y / 2;
    let mut nv12 = vec![0u8; y + uv];
    nv12[y..].fill(128);
    let arr = Uint8Array::from(nv12.as_slice());
    let init = VideoFrameBufferInit::new_with_f64(height, width, VideoPixelFormat::Nv12, 0.0);
    init.set_duration(33_333);
    VideoFrame::new_with_u8_array_and_video_frame_buffer_init(&arr, &init)
        .map_err(|_| JsValue::from_str("VideoFrame::new failed"))
}

#[cfg(feature = "audio")]
fn silence_f32_interleaved(channels: u32, frames: u32) -> Vec<f32> {
    vec![0.0; (frames * channels) as usize]
}

#[cfg(feature = "video")]
fn timestamp_i32(timestamp_us: f64) -> Result<i32, JsValue> {
    timestamp_us_to_i32(timestamp_us)
        .ok_or_else(|| JsValue::from_str("timestamp does not fit i32 microseconds"))
}

/// Solid-luma NV12 frame at an explicit `width`x`height`x`timestamp` — generalizes
/// [`black_nv12_frame`] (which is fixed at 64x64, luma 0, timestamp 0) for
/// [`encode_video_frames`]'s multi-frame, arbitrary-codec path.
#[cfg(feature = "video")]
fn nv12_frame_at(
    width: u32,
    height: u32,
    luma: u8,
    timestamp_us: f64,
) -> Result<VideoFrame, JsValue> {
    let y = (width * height) as usize;
    let uv = y / 2;
    let mut nv12 = vec![0u8; y + uv];
    nv12[..y].fill(luma);
    nv12[y..].fill(128);
    let arr = Uint8Array::from(nv12.as_slice());
    let init =
        VideoFrameBufferInit::new_with_f64(height, width, VideoPixelFormat::Nv12, timestamp_us);
    VideoFrame::new_with_u8_array_and_video_frame_buffer_init(&arr, &init)
        .map_err(|_| JsValue::from_str("VideoFrame::new failed"))
}

/// Multi-frame encode result before being split into [`EncodedVideoChunks`]'s parallel
/// vecs: `(timestamp_us, is_keyframe, payload)` per chunk.
type PendingChunks = Rc<RefCell<Vec<(f64, bool, Vec<u8>)>>>;

/// Encode `lumas.len()` solid-luma NV12 frames with `codec`/`width`/`height`.
///
/// Uses `timestamps_us[i]` (microseconds) as each frame's `WebCodecs` timestamp, and returns
/// every encoded chunk (not just the first). Generalizes [`encode_one_h264_frame_via`] to
/// arbitrary codec + frame count for multi-frame pipelines (trim/splice E2E) — see
/// `tools/e2e-web/tests/decode-trim-splice.spec.ts`.
#[cfg(feature = "video")]
#[wasm_bindgen]
pub async fn encode_video_frames(
    codec: String,
    width: u32,
    height: u32,
    bitrate_bps: u32,
    lumas: Vec<u8>,
    timestamps_us: Vec<f64>,
) -> Result<EncodedVideoChunks, JsValue> {
    if lumas.len() != timestamps_us.len() {
        return Err(JsValue::from_str("lumas/timestamps_us length mismatch"));
    }

    let chunks: PendingChunks = Rc::new(RefCell::new(Vec::new()));
    // clone: Rc callback share
    let chunks_cb = chunks.clone();
    let copy_err: Rc<RefCell<Option<JsValue>>> = Rc::new(RefCell::new(None));
    // clone: Rc callback share
    let copy_err_cb = copy_err.clone();
    let description: Rc<RefCell<Option<Vec<u8>>>> = Rc::new(RefCell::new(None));
    // clone: Rc callback share
    let description_cb = description.clone();
    let output = Closure::wrap(
        Box::new(move |chunk: EncodedVideoChunk, metadata: JsValue| {
            let mut buf = vec![0u8; chunk.byte_length() as usize];
            if chunk.copy_to_with_u8_slice(&mut buf).is_err() {
                *copy_err_cb.borrow_mut() =
                    Some(JsValue::from_str("EncodedVideoChunk::copy_to failed"));
                return;
            }
            let is_key = chunk.type_() == EncodedVideoChunkType::Key;
            chunks_cb
                .borrow_mut()
                .push((chunk.timestamp(), is_key, buf));

            if description_cb.borrow().is_none() {
                if let Some(desc) = decoder_config_description(&metadata) {
                    *description_cb.borrow_mut() = Some(desc);
                }
            }
        }) as Box<dyn FnMut(EncodedVideoChunk, JsValue)>,
    );
    let enc_err: Rc<RefCell<Option<JsValue>>> = Rc::new(RefCell::new(None));
    // clone: Rc callback share
    let enc_err_cb = enc_err.clone();
    let error = Closure::wrap(Box::new(move |e: JsValue| {
        *enc_err_cb.borrow_mut() = Some(e);
    }) as Box<dyn FnMut(JsValue)>);
    let init = VideoEncoderInit::new(
        error.as_ref().unchecked_ref(),
        output.as_ref().unchecked_ref(),
    );
    let enc = VideoEncoder::new(&init).map_err(|_| JsValue::from_str("VideoEncoder::new"))?;
    error.forget();
    output.forget();

    let cfg = WebVideoEncoderConfig::new(&codec, height, width);
    cfg.set_width(width);
    cfg.set_height(height);
    cfg.set_bitrate(bitrate_bps);
    cfg.set_framerate(30.0);
    enc.configure(&cfg)
        .map_err(|_| JsValue::from_str("VideoEncoder::configure"))?;

    for (&luma, &ts) in lumas.iter().zip(timestamps_us.iter()) {
        // Validate up front even though the frame itself only needs an i32-range timestamp
        // internally (VideoFrameBufferInit takes f64) — keeps failure symmetric with the
        // decode side's EncodedVideoChunk timestamp, which is i32-constrained by WebCodecs.
        timestamp_i32(ts)?;
        let frame = nv12_frame_at(width, height, luma, ts)?;
        enc.encode(&frame)
            .map_err(|_| JsValue::from_str("VideoEncoder::encode"))?;
        frame.close();
    }
    JsFuture::from(enc.flush()).await?;
    let _ = enc.close(); // see encode_frame_via's close() comment — same hygiene fix
    take_js_err(&enc_err)?;
    take_js_err(&copy_err)?;

    let mut collected = chunks.borrow_mut();
    let mut timestamps_us_out = Vec::with_capacity(collected.len());
    let mut keyframes = Vec::with_capacity(collected.len());
    let mut payloads = Vec::with_capacity(collected.len());
    for (ts, is_key, data) in collected.drain(..) {
        timestamps_us_out.push(ts);
        keyframes.push(is_key);
        payloads.push(data);
    }
    let description = description.borrow_mut().take();
    Ok(EncodedVideoChunks::new(
        timestamps_us_out,
        keyframes,
        payloads,
        description,
    ))
}

/// Read `metadata.decoderConfig.description` off a `VideoEncoder` output callback's second
/// argument, when present — `WebCodecs` only sets `decoderConfig` on the chunk(s) where the
/// config is (re-)established (typically the first chunk), and only some codecs need an
/// out-of-band description at all (H.264's `avcC` SPS/PPS record; VP8/VP9/AV1 are self-
/// describing in-band and normally have none).
#[cfg(feature = "video")]
fn decoder_config_description(metadata: &JsValue) -> Option<Vec<u8>> {
    if metadata.is_undefined() || metadata.is_null() {
        return None;
    }
    let metadata: &web_sys::EncodedVideoChunkMetadata = metadata.unchecked_ref();
    let decoder_config = metadata.get_decoder_config()?;
    let description = decoder_config.get_description()?;
    Some(Uint8Array::new(&description).to_vec())
}

#[cfg(all(feature = "audio", feature = "video"))]
fn mux_av_chunks(video: &[u8], audio: &[u8]) -> Result<Vec<u8>, JsValue> {
    let mut open = Muxer::with_fragment_batch(1);
    open.add_track(Track {
        id: 0,
        codec: Codec::H264,
        time_base: Rational::new(1, 90_000),
        width: 64,
        height: 64,
        extra_data: Bytes::new(),
    })
    .map_err(|e| iso_err(&e))?;
    open.add_track(Track {
        id: 1,
        codec: Codec::Aac,
        time_base: Rational::new(1, 48_000),
        width: 0,
        height: 0,
        extra_data: Bytes::from_static(&[0x11, 0x90]),
    })
    .map_err(|e| iso_err(&e))?;
    let mut mux = open.begin();
    mux.push_packet(&Sample {
        stream_id: 0,
        pts: 0,
        dts: 0,
        duration: 3000,
        is_keyframe: true,
        is_discard: false,
        payload: Bytes::copy_from_slice(video),
    })
    .map_err(|e| iso_err(&e))?;
    mux.push_packet(&Sample {
        stream_id: 1,
        pts: 0,
        dts: 0,
        duration: 1024,
        is_keyframe: true,
        is_discard: false,
        payload: Bytes::copy_from_slice(audio),
    })
    .map_err(|e| iso_err(&e))?;
    mux.flush();
    let mut bytes = Vec::new();
    mux.poll_bytes(&mut bytes);
    if bytes.len() < 12 || &bytes[4..8] != b"ftyp" {
        return Err(JsValue::from_str("invalid fMP4 output"));
    }
    Ok(bytes)
}

/// Video-only fMP4 mux (one H.264 track) — companion to [`mux_av_chunks`] for the
/// WebGPU-canvas smoke path, which has no audio chunk to interleave.
#[cfg(feature = "video")]
fn mux_video_chunk(video: &[u8]) -> Result<Vec<u8>, JsValue> {
    let mut open = Muxer::with_fragment_batch(1);
    open.add_track(Track {
        id: 0,
        codec: Codec::H264,
        time_base: Rational::new(1, 90_000),
        width: 64,
        height: 64,
        extra_data: Bytes::new(),
    })
    .map_err(|e| iso_err(&e))?;
    let mut mux = open.begin();
    mux.push_packet(&Sample {
        stream_id: 0,
        pts: 0,
        dts: 0,
        duration: 3000,
        is_keyframe: true,
        is_discard: false,
        payload: Bytes::copy_from_slice(video),
    })
    .map_err(|e| iso_err(&e))?;
    mux.flush();
    let mut bytes = Vec::new();
    mux.poll_bytes(&mut bytes);
    if bytes.len() < 12 || &bytes[4..8] != b"ftyp" {
        return Err(JsValue::from_str("invalid fMP4 output"));
    }
    Ok(bytes)
}

/// Map facade video config to a browser label (smoke / docs).
#[cfg(feature = "video")]
#[wasm_bindgen]
pub fn video_config_label(_config: &WebVideoOpenConfig) -> String {
    "h264".to_string()
}

/// Map facade audio config to a browser label (smoke / docs).
#[cfg(feature = "audio")]
#[wasm_bindgen]
pub fn audio_config_label(_config: &WebAudioOpenConfig) -> String {
    "aac".to_string()
}

/// Demux packet count from fMP4 bytes (smoke helper for Playwright).
#[wasm_bindgen]
pub fn fmp4_packet_count(bytes: &[u8]) -> u32 {
    let mut demux = Demuxer::new();
    demux.push_bytes(bytes);
    let mut n = 0u32;
    while demux.poll_packet().is_some() {
        n += 1;
    }
    n
}
