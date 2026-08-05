/*
 * pipeline.h — mediaway-ffi: C ABI facade over Mediaway's auto video/audio
 * encode -> fragmented MP4 convenience layer, auto video decode
 * (adr/0004-auto-decode-c-abi.md), a capture-to-encode bridge
 * (adr/0005-capture-encode-bridge-c-abi.md) that wires device.h capture handles
 * directly into an encode session, and Opus audio decode + Opus wired into the
 * audio encode surface (adr/pipeline/0006-audio-decode-c-abi.md).
 *
 * Hand-written (not cbindgen-generated) — see adr/0001-auto-encode-c-abi.md §8.
 * Design rules: docs/spec/c-ffi.md (ADR-0004).
 *
 * Ownership summary (see adr/0001-auto-encode-c-abi.md §6, adr/0002-gpu-frame-input-c-abi.md
 * §2 for the full rationale):
 *   - mediaway_auto_video_encode_config_t is a plain value struct: no heap
 *     allocation, no free function, passed/returned by value.
 *   - mediaway_video_frame_t is tagged by storage_kind: MEDIAWAY_VIDEO_FRAME_STORAGE_CPU
 *     carries raw_bytes, a caller-owned borrow valid for the duration of the call
 *     only. MEDIAWAY_VIDEO_FRAME_STORAGE_GPU carries gpu_buffer, a BORROWED handle
 *     aliasing the caller's own GPU texture — this library never frees it and never
 *     calls Release() on it; see the GPU HAZARDS section below. This is the OPPOSITE
 *     ownership direction from mediaway-device-ffi's mediaway_device_video_frame_t
 *     (a borrowed OUTPUT there) — same struct name would have been a real
 *     field-shape collision, so these are distinctly-named types.
 *   - mediaway_encode_session_finish's out_data is a library-owned buffer; release
 *     it with mediaway_pipeline_ffi_buffer_free.
 *
 * Handle consumption (non-obvious, stated here and on each function below):
 *   - mediaway_encode_session_open takes ownership of `encoder` UNCONDITIONALLY —
 *     on success AND on failure — because EncodeSession::open takes its encoder by
 *     value in Rust. Do not call mediaway_auto_encoder_close on `encoder` after
 *     calling this function, regardless of the returned status (double-free).
 *   - mediaway_encode_session_finish takes ownership of `session` UNCONDITIONALLY
 *     for the same reason (EncodeSession::finish takes `self` by value). Do not
 *     call mediaway_encode_session_close on `session` afterward.
 *
 * v1 scope, revised: mediaway_auto_video_encode_config_t now also carries
 * gpu_device (adr/0002-gpu-frame-input-c-abi.md §1) — a real device opts the
 * session into the Zero-Copy/GPU-copy input path at open time.
 * backend/max_path_class stay deferred, always resolving to Auto/the Rust-level
 * default (CpuUpload, which already permits ZeroCopy/GpuCopy) underneath. See
 * adr/0001-auto-encode-c-abi.md §1.
 *
 * GPU HAZARDS (mediaway_video_frame_t with storage_kind == GPU only —
 * adr/0002-gpu-frame-input-c-abi.md §2, NOT hidden):
 *   - gpu_buffer's native_a (e.g. an ID3D11Texture2D*) is a NON-OWNING, BORROWED
 *     pointer — the CALLER retains the only owning reference. This library does
 *     NOT call Release() on it and does not extend its lifetime past the call.
 *   - Read window: the texture must remain valid and unmodified by the caller for
 *     the duration of the mediaway_encode_session_write_frame call only — the
 *     encoder backend reads (and may issue a GPU-side copy/upload from) it
 *     synchronously within that call. Once the call returns, the caller is free
 *     to reuse or release the texture.
 *   - ID3D11Device immediate-context concurrency: if the encoder backend issues
 *     immediate-context GPU commands against the SAME ID3D11Device passed as
 *     gpu_device, and the caller's own code also issues immediate-context
 *     commands on that device concurrently from another thread, either enable
 *     ID3D11Multithread::SetMultithreadProtected(TRUE) on the device before
 *     passing it in, or confine the caller's own immediate-context use to avoid
 *     overlapping write_frame calls — the standard "ID3D11 immediate context is
 *     not safe for concurrent multi-thread submission" hazard.
 *
 * Thread safety: every handle (mediaway_auto_encoder_t*, mediaway_encode_session_t*)
 * is thread-confined by convention, not internally synchronized. A handle may be
 * moved to another thread, but calling two functions on the SAME handle
 * concurrently from different threads without external synchronization is a data
 * race (undefined behavior), not merely wrong output.
 *
 * This is a distinct, independently-numbered status enum and independently-named
 * buffer-free function from mediaway-container-ffi's <mediaway/container.h> — see
 * adr/0001-auto-encode-c-abi.md §2 and § Buffer-free naming for why the two are
 * not unified yet.
 */

#ifndef MEDIAWAY_PIPELINE_H
#define MEDIAWAY_PIPELINE_H

#define MEDIAWAY_PIPELINE_FFI_ABI_VERSION 5 /* bump on any breaking change; pre-1.0, no stability promise */

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include "common.h"

#ifdef __cplusplus
extern "C" {
#endif

/* ── Opaque handles ──────────────────────────────────────────────────────────────── */

/* No member list: layout is private to the Rust implementation and may change without
 * notice pre-1.0. Always access through the functions below. */
typedef struct mediaway_auto_encoder mediaway_auto_encoder_t;
typedef struct mediaway_encode_session mediaway_encode_session_t;
typedef struct mediaway_audio_encode_session mediaway_audio_encode_session_t; /* adr/0003 — the session IS the encoder; no intermediate handle */
typedef struct mediaway_decode_session mediaway_decode_session_t; /* adr/0004 — the session IS the decoder; no intermediate handle */
typedef struct mediaway_audio_decode_session mediaway_audio_decode_session_t; /* adr/pipeline/0006 — wraps OpusDecoder directly, no intermediate handle */

/* Forward declarations only (adr/0005-capture-encode-bridge-c-abi.md): the real
 * definitions live in device.h. Guarded so this header still compiles standalone
 * AND co-includes cleanly with device.h — same pattern common.h's types use. The
 * capture-to-encode bridge functions below are only actually usable once linked
 * against real Camera/Desktop capture handles opened via device.h's own functions. */
#ifndef MEDIAWAY_CAMERA_CAPTURE_T_DEFINED
#define MEDIAWAY_CAMERA_CAPTURE_T_DEFINED
typedef struct mediaway_camera_capture mediaway_camera_capture_t;
#endif
#ifndef MEDIAWAY_DESKTOP_CAPTURE_T_DEFINED
#define MEDIAWAY_DESKTOP_CAPTURE_T_DEFINED
typedef struct mediaway_desktop_capture mediaway_desktop_capture_t;
#endif

/* ── Status codes ────────────────────────────────────────────────────────────────── */

typedef enum mediaway_pipeline_status {
    MEDIAWAY_PIPELINE_STATUS_OK                      = 0,
    MEDIAWAY_PIPELINE_STATUS_INVALID_ARGUMENT        = 1,  /* null pointer, mismatched ptr/len */
    MEDIAWAY_PIPELINE_STATUS_HANDLE_POISONED         = 2,  /* a previous call on this handle panicked */
    MEDIAWAY_PIPELINE_STATUS_NO_BACKEND              = 3,  /* no encode backend compiled in — expected/graceful */
    MEDIAWAY_PIPELINE_STATUS_UNSUPPORTED             = 4,  /* bad codec/pixel-format/geometry — context-dependent, not always graceful */
    MEDIAWAY_PIPELINE_STATUS_INVALID_INPUT           = 5,  /* bad dimensions, rates, or frame metadata */
    MEDIAWAY_PIPELINE_STATUS_ENCODER_BACKEND_FAILURE = 6,  /* OS/API failure inside the encoder backend */
    MEDIAWAY_PIPELINE_STATUS_ENCODER_CLOSED          = 7,  /* session already finished or not open */
    MEDIAWAY_PIPELINE_STATUS_MUX_INVALID_TRACK       = 8,  /* muxer rejected the encoder's stream info */
    MEDIAWAY_PIPELINE_STATUS_MUX_INVALID_PACKET      = 9,  /* packet does not match the registered track */
    MEDIAWAY_PIPELINE_STATUS_MUX_INVALID_DATA        = 10, /* malformed container data */
    MEDIAWAY_PIPELINE_STATUS_UNKNOWN_ERROR           = 11, /* reserved for a future error variant */
    MEDIAWAY_PIPELINE_STATUS_INTERNAL_PANIC          = 12, /* this call caught a Rust panic; the handle is now poisoned */
    MEDIAWAY_PIPELINE_STATUS_DECODER_BACKEND_FAILURE = 13, /* OS/API failure inside the decoder backend */
    MEDIAWAY_PIPELINE_STATUS_DECODER_CLOSED          = 14, /* decode session already finished or not open */
} mediaway_pipeline_status_t;

/* ── Shared value types ──────────────────────────────────────────────────────────── */

/* mediaway_rational_t, mediaway_pixel_format_t, mediaway_sample_format_t, and the GPU
 * device/buffer handle types below all come from common.h. */

/* Distinct type name from mediaway_codec_kind_t (container.h), but numeric
 * values are deliberately mirrored 1:1 — both wrap the same shared Rust type
 * (mediaway_common::CodecKind) end-to-end. Passing a non-video codec (AAC..RAW_AUDIO)
 * to mediaway_auto_video_encode_config_new is a runtime
 * MEDIAWAY_PIPELINE_STATUS_INVALID_ARGUMENT, not a compile-time restriction. */
typedef enum mediaway_pipeline_codec_kind {
    MEDIAWAY_PIPELINE_CODEC_H264      = 0,
    MEDIAWAY_PIPELINE_CODEC_HEVC      = 1,
    MEDIAWAY_PIPELINE_CODEC_AV1       = 2,
    MEDIAWAY_PIPELINE_CODEC_VP9       = 3,
    MEDIAWAY_PIPELINE_CODEC_AAC       = 4,
    MEDIAWAY_PIPELINE_CODEC_OPUS      = 5,
    MEDIAWAY_PIPELINE_CODEC_MP3       = 6,
    MEDIAWAY_PIPELINE_CODEC_VORBIS    = 7,
    MEDIAWAY_PIPELINE_CODEC_WEBVTT    = 8,
    MEDIAWAY_PIPELINE_CODEC_TX3G      = 9,
    MEDIAWAY_PIPELINE_CODEC_RAW_VIDEO = 10,
    MEDIAWAY_PIPELINE_CODEC_RAW_AUDIO = 11,
} mediaway_pipeline_codec_kind_t; /* pre-1.0 — values may be renumbered */

/* mediaway_video_frame_t's GPU-storage input (mediaway_gpu_buffer_handle_t) is
 * BORROWED, not owned. See the file header's GPU HAZARDS section for the full
 * read-window / immediate-context contract. Opposite ownership direction from
 * device.h's identically-shaped type (that one is a borrowed OUTPUT; this one is a
 * borrowed INPUT). */

/* Plain value type; no free function. gpu_device (adr/0002-gpu-frame-input-c-abi.md
 * §1) opts the session into the Zero-Copy/GPU-copy input path at open time;
 * MEDIAWAY_GPU_DEVICE_NONE (the zero value) keeps the existing CPU-only behavior.
 * backend/max_path_class stay deferred (see the file header comment above). */
typedef struct mediaway_auto_video_encode_config {
    mediaway_pipeline_codec_kind_t codec;
    uint32_t width;
    uint32_t height;
    mediaway_rational_t time_base;
    uint32_t bitrate_bps;                 /* 0 = backend default */
    mediaway_pixel_format_t pixel_format;
    mediaway_gpu_device_handle_t gpu_device; /* MEDIAWAY_GPU_DEVICE_NONE for CPU-only */
} mediaway_auto_video_encode_config_t;

/* mediaway_video_frame_storage_kind_t comes from common.h (CPU: raw_bytes/raw_bytes_len
 * valid, gpu_buffer unused; GPU: gpu_buffer valid, raw_bytes == NULL, raw_bytes_len == 0). */

/* Input to mediaway_encode_session_write_frame — borrowed view, valid for the call
 * only. storage_kind decides which of raw_bytes/gpu_buffer is read (see the file
 * header's ownership summary and GPU HAZARDS section). */
typedef struct mediaway_video_frame {
    int64_t pts;
    uint64_t duration;             /* 0 if unknown */
    uint32_t width;
    uint32_t height;
    mediaway_pixel_format_t pixel_format;
    mediaway_video_frame_storage_kind_t storage_kind;
    const uint8_t *raw_bytes;      /* CPU only; borrowed; NULL iff raw_bytes_len == 0 or storage_kind == GPU */
    size_t raw_bytes_len;          /* CPU only; 0 whenever storage_kind == GPU */
    mediaway_gpu_buffer_handle_t gpu_buffer; /* GPU only; BORROWED, zeroed whenever storage_kind == CPU */
} mediaway_video_frame_t;

/* ── ABI version ─────────────────────────────────────────────────────────────────── */

/* Runtime counterpart to MEDIAWAY_PIPELINE_FFI_ABI_VERSION, for consumers that load
 * this library dynamically and never compile against this header. */
uint32_t mediaway_pipeline_ffi_abi_version(void);

/* ── Config (plain value struct, no handle, no free) ─────────────────────────────── */

/* Explicit size and codec — resolution comes from the app, not a named preset.
 * Defaults bitrate_bps to 0 (backend default), pixel_format to NV12, and gpu_device
 * to MEDIAWAY_GPU_DEVICE_NONE (CPU-only). Callers wanting the GPU input path set
 * .gpu_device on the returned value afterward. */
mediaway_auto_video_encode_config_t mediaway_auto_video_encode_config_new(
    mediaway_pipeline_codec_kind_t codec, uint32_t width, uint32_t height,
    mediaway_rational_t time_base);

/* Sugar over mediaway_auto_video_encode_config_new(MEDIAWAY_PIPELINE_CODEC_H264, ...).
 * Kept for the aspirational example's ergonomics; the general form above also exists
 * since H.264 is not the only real codec the Windows auto backend resolves today. */
mediaway_auto_video_encode_config_t mediaway_auto_video_encode_config_h264(
    uint32_t width, uint32_t height, mediaway_rational_t time_base);

/* ── Auto encoder (intermediate handle) ──────────────────────────────────────────── */

/* Open the best available video encoder for `config` on the current platform.
 * MEDIAWAY_PIPELINE_STATUS_NO_BACKEND ("no backend compiled in here") is an expected,
 * graceful outcome, not a bug — check for it and exit cleanly rather than treating it
 * like a fatal error. *out_encoder is NULL on any non-OK status (a normal Err, or a
 * caught panic). */
mediaway_pipeline_status_t mediaway_auto_encoder_open(
    const mediaway_auto_video_encode_config_t *config,
    mediaway_auto_encoder_t **out_encoder);

/* Close and free an auto-encoder handle WITHOUT ever calling
 * mediaway_encode_session_open on it. Only for abandoning an opened encoder on an
 * early-abort path — do not call this after mediaway_encode_session_open has already
 * consumed `encoder` (double-free). Always safe to call, including with encoder == NULL. */
void mediaway_auto_encoder_close(mediaway_auto_encoder_t *encoder);

/* ── Encode session ───────────────────────────────────────────────────────────────── */

/* Register `encoder`'s stream as an MP4 track and begin streaming. Consumes
 * `encoder` UNCONDITIONALLY — success or failure — do not call
 * mediaway_auto_encoder_close on it afterward regardless of the returned status. */
mediaway_pipeline_status_t mediaway_encode_session_open(
    mediaway_auto_encoder_t *encoder, mediaway_encode_session_t **out_session);

/* Push one frame and drain any packets it produces into the muxer. `frame`'s
 * storage_kind decides which of raw_bytes/gpu_buffer is read; both are
 * caller-owned borrows, valid for the duration of this call only — see the file
 * header's ownership summary and GPU HAZARDS section for the GPU case. */
mediaway_pipeline_status_t mediaway_encode_session_write_frame(
    mediaway_encode_session_t *session, const mediaway_video_frame_t *frame);

/* Flush the encoder and muxer, returning the complete fMP4 byte stream. Consumes
 * `session` UNCONDITIONALLY — do not call mediaway_encode_session_close on it
 * afterward regardless of the returned status. Release *out_data with
 * mediaway_pipeline_ffi_buffer_free. */
mediaway_pipeline_status_t mediaway_encode_session_finish(
    mediaway_encode_session_t *session, uint8_t **out_data, size_t *out_len);

/* Abandon a session WITHOUT finishing it (no flush, no valid MP4 output) — added for
 * the same resource-cleanup symmetry as mediaway_auto_encoder_close. Always safe to
 * call, including on a poisoned handle or with session == NULL. Do not call this
 * after mediaway_encode_session_finish has already consumed `session`. */
void mediaway_encode_session_close(mediaway_encode_session_t *session);

/* ── Shared free ─────────────────────────────────────────────────────────────────── */

/* Free a buffer returned by mediaway_encode_session_finish. Distinctly named from
 * mediaway-container-ffi's mediaway_buffer_free — see the file header comment above. */
void mediaway_pipeline_ffi_buffer_free(uint8_t *data, size_t len);

/* ── Audio encode (adr/0003-auto-audio-encode-c-abi.md) ─────────────────────────── */

/* mediaway_sample_format_t comes from common.h. Only F32 is accepted by the real
 * Windows backend today; the other variants exist so the enum can be extended without
 * a version bump. */

/* Config for mediaway_audio_encoder_open — plain value struct, no handle, no heap
 * allocation, no free function. codec is AAC (Windows only) or Opus (cross-platform,
 * mediaway-sw) today — any other kind is a runtime MEDIAWAY_PIPELINE_STATUS_UNSUPPORTED;
 * sample_format is F32 today (adr/pipeline/0006-audio-decode-c-abi.md § Encode side). */
typedef struct mediaway_audio_encode_config {
    mediaway_pipeline_codec_kind_t codec;  /* output codec: AAC or Opus */
    uint32_t sample_rate;                  /* input sample rate in Hz, non-zero */
    uint16_t channels;                     /* input channel count, non-zero */
    mediaway_sample_format_t sample_format; /* input PCM format (F32 today) */
    mediaway_rational_t time_base;         /* timestamp timebase for pushed frames / polled packets */
    uint32_t bitrate_bps;                  /* target bitrate; 0 = backend default (128 kbps) */
} mediaway_audio_encode_config_t;

/* Build a stereo AAC config (F32 input, backend-default bitrate) — the only
 * combination the real Windows backend accepts today, kept as the ergonomic sugar.
 * The general form exists as the struct itself; no general constructor is exported. */
mediaway_audio_encode_config_t mediaway_audio_encode_config_aac(
    uint32_t sample_rate, mediaway_rational_t time_base);

/* Build an Opus config (F32 input, backend-default bitrate, cross-platform
 * mediaway-sw backend). Unlike the AAC sugar, channels is caller-chosen — Opus
 * voice use is commonly mono (adr/pipeline/0006-audio-decode-c-abi.md § 1). */
mediaway_audio_encode_config_t mediaway_audio_encode_config_opus(
    uint32_t sample_rate, uint16_t channels, mediaway_rational_t time_base);

/* Input to mediaway_audio_encode_session_push_pcm — BORROWED view, valid for the
 * call only (same ownership direction as mediaway_video_frame_t's raw_bytes). */
typedef struct mediaway_audio_frame_view {
    int64_t pts;               /* presentation timestamp in the stream timebase */
    uint64_t duration;         /* duration in timebase units (0 if unknown) */
    uint32_t sample_rate;      /* sample rate in Hz */
    uint16_t channels;         /* channel count */
    mediaway_sample_format_t sample_format; /* PCM sample format (F32 today) */
    const uint8_t *data;       /* BORROWED interleaved PCM bytes; NULL iff data_len == 0 */
    size_t data_len;           /* length of data in bytes */
} mediaway_audio_frame_view_t;

/* Output of mediaway_audio_encode_session_poll_packet — OWNED; release with
 * mediaway_pipeline_ffi_packet_free. No stream_id: the caller assigns the muxer
 * track id when pushing into their own container muxer (the session does not mux). */
typedef struct mediaway_audio_packet {
    int64_t pts;           /* presentation timestamp in the stream timebase */
    int64_t dts;           /* decode timestamp in the stream timebase */
    uint64_t duration;     /* duration in timebase units */
    bool is_keyframe;      /* random access point */
    bool is_discard;       /* outside the active edit window; decoders may skip */
    uint8_t *payload;      /* OWNED AAC bitstream bytes; NULL after packet_free */
    size_t payload_len;    /* length of payload in bytes */
} mediaway_audio_packet_t;

/* Output of mediaway_audio_encode_session_stream_info — OWNED; release with
 * mediaway_pipeline_ffi_stream_info_free. extra_data is the AudioSpecificConfig
 * (raw, MP4 esds-ready) — copy it into mediaway_audio_track_info_t (container.h)
 * when registering the muxer track. */
typedef struct mediaway_audio_stream_info {
    mediaway_pipeline_codec_kind_t codec; /* output codec (AAC today) */
    mediaway_rational_t time_base;        /* stream timebase */
    uint32_t sample_rate;                 /* Hz; 0 when not yet known */
    uint16_t channels;                    /* channel count; 0 when not yet known */
    uint8_t *extra_data;                  /* OWNED codec config (AudioSpecificConfig); NULL after stream_info_free */
    size_t extra_data_len;                /* length of extra_data in bytes */
} mediaway_audio_stream_info_t;

/* Open the best available audio encoder for `config`. The returned handle IS the
 * encode session — no intermediate handle, so no consumption trap (adr/0003 §
 * Decision: audio has no internal muxer). MEDIAWAY_PIPELINE_STATUS_NO_BACKEND is
 * an expected graceful outcome on platforms without an audio backend; check for it
 * and exit cleanly. *out_session is NULL on any non-OK status (a normal Err, or a
 * caught panic). */
mediaway_pipeline_status_t mediaway_audio_encoder_open(
    const mediaway_audio_encode_config_t *config,
    mediaway_audio_encode_session_t **out_session);

/* Query the session's stream metadata: codec, timebase, sample rate, channel count,
 * and the codec config (extra_data — the AudioSpecificConfig an MP4 audio track
 * needs to be playable). The AudioSpecificConfig materializes only after the first
 * PCM frame is pushed, so call this after pushing, before muxing. OWNED output:
 * release *out_info with mediaway_pipeline_ffi_stream_info_free. */
mediaway_pipeline_status_t mediaway_audio_encode_session_stream_info(
    mediaway_audio_encode_session_t *session,
    mediaway_audio_stream_info_t *out_info);

/* Push one PCM buffer. `frame` is a BORROWED view valid for the duration of this
 * call only — the encoder copies synchronously (same cost class as the video
 * CPU-upload path). */
mediaway_pipeline_status_t mediaway_audio_encode_session_push_pcm(
    mediaway_audio_encode_session_t *session,
    const mediaway_audio_frame_view_t *frame);

/* Pull the next encoded packet, if one is ready. *out_has_packet == true: *out_packet
 * is an OWNED output — release it with mediaway_pipeline_ffi_packet_free. false is
 * a valid "nothing ready" result, not an error. */
mediaway_pipeline_status_t mediaway_audio_encode_session_poll_packet(
    mediaway_audio_encode_session_t *session,
    mediaway_audio_packet_t *out_packet, bool *out_has_packet);

/* Signal end-of-input; drain the remaining packets with
 * mediaway_audio_encode_session_poll_packet afterwards. */
mediaway_pipeline_status_t mediaway_audio_encode_session_flush(
    mediaway_audio_encode_session_t *session);

/* Close and free an audio encode session. Always safe to call, including with
 * session == NULL — this surface has no handle-consumption trap (adr/0003). */
void mediaway_audio_encode_session_close(mediaway_audio_encode_session_t *session);

/* Free a packet returned by mediaway_audio_encode_session_poll_packet. Nulls the
 * payload fields afterward, making a double-free a visible no-op. Always safe to
 * call, including with packet == NULL. */
void mediaway_pipeline_ffi_packet_free(mediaway_audio_packet_t *packet);

/* Free stream info returned by mediaway_audio_encode_session_stream_info. Nulls the
 * extra_data fields afterward, making a double-free a visible no-op. Always safe to
 * call, including with info == NULL. */
void mediaway_pipeline_ffi_stream_info_free(mediaway_audio_stream_info_t *info);

/* ── Audio decode (adr/pipeline/0006-audio-decode-c-abi.md) ─────────────────────── */

/* Config for mediaway_audio_decode_session_open. codec is Opus only today (any other
 * kind is a runtime MEDIAWAY_PIPELINE_STATUS_UNSUPPORTED). No extra_data field —
 * unlike video codecs, Opus needs no out-of-band codec config to open a decoder.
 * Output PCM is always F32 (opus_decode_float). */
typedef struct mediaway_audio_decode_config {
    mediaway_pipeline_codec_kind_t codec; /* input codec (Opus today) */
    uint32_t sample_rate;                 /* Hz, non-zero */
    uint16_t channels;                    /* non-zero */
    mediaway_rational_t time_base;        /* frame duration; also the decode buffer's per-frame sample cap */
} mediaway_audio_decode_config_t;

/* Build an Opus decode config for `sample_rate`/`channels`/`time_base`. */
mediaway_audio_decode_config_t mediaway_audio_decode_config_opus(
    uint32_t sample_rate, uint16_t channels, mediaway_rational_t time_base);

/* Output of mediaway_audio_decode_session_poll_frame — OWNED; release with
 * mediaway_decoded_audio_frame_free. New, pipeline-scoped name: distinct ownership
 * direction from mediaway_audio_frame_view_t (borrowed encode *input* there vs.
 * owned decode *output* here), same naming precedent as mediaway_decoded_video_frame_t
 * vs. mediaway_video_frame_t. */
typedef struct mediaway_decoded_audio_frame {
    int64_t pts;
    uint64_t duration;                      /* 0 if unknown */
    uint32_t sample_rate;
    uint16_t channels;
    mediaway_sample_format_t sample_format; /* always F32 for Opus */
    uint8_t *data;                          /* OWNED interleaved PCM; NULL after mediaway_decoded_audio_frame_free */
    size_t data_len;
} mediaway_decoded_audio_frame_t;

/* Open an Opus decode session for `config` — single step, the handle IS the decoder
 * (mirrors mediaway_decode_session_t's video shape; no muxer to wire, no
 * consumption trap). MEDIAWAY_PIPELINE_STATUS_UNSUPPORTED for any codec other than
 * Opus. *out_session is NULL on any non-OK status (a normal Err, or a caught panic). */
mediaway_pipeline_status_t mediaway_audio_decode_session_open(
    const mediaway_audio_decode_config_t *config,
    mediaway_audio_decode_session_t **out_session);

/* Push one compressed Opus packet. Reuses mediaway_decode_packet_view_t (the video
 * decode packet type — already codec-agnostic, see adr/pipeline/0006 §4). An empty
 * payload (NULL or payload_len == 0) is Opus's packet-loss-concealment hint for a
 * lost frame, not an error — pass it whenever a frame is known lost. May produce
 * zero or more frames (drain via mediaway_audio_decode_session_poll_frame). */
mediaway_pipeline_status_t mediaway_audio_decode_session_push_packet(
    mediaway_audio_decode_session_t *session, const mediaway_decode_packet_view_t *packet);

/* Pull the next decoded PCM frame, if any is ready. *out_has_frame == false is a
 * valid "nothing ready" result, not an error. When true, release *out_frame with
 * mediaway_decoded_audio_frame_free. */
mediaway_pipeline_status_t mediaway_audio_decode_session_poll_frame(
    mediaway_audio_decode_session_t *session, mediaway_decoded_audio_frame_t *out_frame,
    bool *out_has_frame);

/* Signal end-of-input; drain remaining frames with
 * mediaway_audio_decode_session_poll_frame afterward. */
mediaway_pipeline_status_t mediaway_audio_decode_session_flush(
    mediaway_audio_decode_session_t *session);

/* Close and free an audio decode-session handle. Always safe to call, including on a
 * poisoned handle or with session == NULL — this surface has no consumption trap. */
void mediaway_audio_decode_session_close(mediaway_audio_decode_session_t *session);

/* Free a frame returned by mediaway_audio_decode_session_poll_frame. Nulls
 * data/data_len afterward, making a double-free a visible no-op. Always safe to
 * call, including with frame == NULL. */
void mediaway_decoded_audio_frame_free(mediaway_decoded_audio_frame_t *frame);

/* ── Video decode (adr/0004-auto-decode-c-abi.md) ────────────────────────────────── */

/* Config for mediaway_decode_session_open. extra_data (AVCC / SPS-PPS codec config)
 * is a BORROWED input, valid for the duration of that call only — required at OPEN
 * time (not supplied via the first pushed packet; see the ADR §1 for why the muxer-
 * track analogy does not hold for the wrapped decoder). NULL/0 opens without a known
 * codec config. GPU output stays deferred — always opens CpuFramesOk internally. */
typedef struct mediaway_auto_video_decode_config {
    mediaway_pipeline_codec_kind_t codec;
    uint32_t width;              /* expected; may be refined from the bitstream */
    uint32_t height;
    mediaway_rational_t time_base;
    mediaway_pixel_format_t pixel_format; /* preferred output format when the backend converts */
    const uint8_t *extra_data;   /* BORROWED; valid for the open call only; NULL iff extra_data_len == 0 */
    size_t extra_data_len;
} mediaway_auto_video_decode_config_t;

/* Input to mediaway_decode_session_push_packet — BORROWED view, valid for the call
 * only. A new, pipeline-scoped type, not reused from container.h's
 * mediaway_packet_view_t (adr/0004 §4). stream_id is accepted but unused by decode. */
typedef struct mediaway_decode_packet_view {
    uint32_t stream_id;    /* unused by decode; kept for call-site symmetry */
    int64_t pts;
    int64_t dts;
    uint64_t duration;
    bool is_keyframe;
    bool is_discard;
    const uint8_t *payload; /* BORROWED; valid for the call only; NULL iff payload_len == 0 */
    size_t payload_len;
} mediaway_decode_packet_view_t;

/* Output of mediaway_decode_session_poll_frame — OWNED; release with
 * mediaway_decoded_video_frame_free. CPU-only (no storage_kind/gpu_buffer — GPU
 * decode output is deferred, adr/0004 §1/§5). */
typedef struct mediaway_decoded_video_frame {
    int64_t pts;
    uint64_t duration;      /* 0 if unknown */
    uint32_t width;
    uint32_t height;
    mediaway_pixel_format_t pixel_format;
    uint8_t *data;           /* OWNED; NULL after mediaway_decoded_video_frame_free */
    size_t data_len;
} mediaway_decoded_video_frame_t;

/* Build a decode config for `codec` at `width`x`height`/`time_base`. `extra_data`
 * must remain valid until mediaway_decode_session_open is called. Defaults
 * pixel_format to NV12. */
mediaway_auto_video_decode_config_t mediaway_auto_video_decode_config_new(
    mediaway_pipeline_codec_kind_t codec, uint32_t width, uint32_t height,
    mediaway_rational_t time_base, const uint8_t *extra_data, size_t extra_data_len);

/* Open the best available video decoder for `config` — single step, the handle IS
 * the decoder (like audio encode; no muxer to wire, so no consumption trap).
 * MEDIAWAY_PIPELINE_STATUS_NO_BACKEND is an expected graceful outcome; check for it
 * and exit cleanly. *out_session is NULL on any non-OK status. */
mediaway_pipeline_status_t mediaway_decode_session_open(
    const mediaway_auto_video_decode_config_t *config,
    mediaway_decode_session_t **out_session);

/* Push one compressed packet. May produce zero or more frames (drain via
 * mediaway_decode_session_poll_frame). `packet->payload` is a caller-owned borrow,
 * valid for the call only. */
mediaway_pipeline_status_t mediaway_decode_session_push_packet(
    mediaway_decode_session_t *session, const mediaway_decode_packet_view_t *packet);

/* Pull the next decoded frame, if any is ready. *out_has_frame == false is a valid
 * "nothing ready" result, not an error. When true, release *out_frame with
 * mediaway_decoded_video_frame_free. */
mediaway_pipeline_status_t mediaway_decode_session_poll_frame(
    mediaway_decode_session_t *session, mediaway_decoded_video_frame_t *out_frame,
    bool *out_has_frame);

/* Signal end-of-input; drain remaining frames with mediaway_decode_session_poll_frame
 * afterward. */
mediaway_pipeline_status_t mediaway_decode_session_flush(
    mediaway_decode_session_t *session);

/* Close and free a decode-session handle. Always safe to call, including on a
 * poisoned handle or with session == NULL — this surface has no consumption trap. */
void mediaway_decode_session_close(mediaway_decode_session_t *session);

/* Free a frame returned by mediaway_decode_session_poll_frame. Nulls data/data_len
 * afterward, making a double-free a visible no-op. Always safe to call, including
 * with frame == NULL. */
void mediaway_decoded_video_frame_free(mediaway_decoded_video_frame_t *frame);

/* ── Capture-to-encode bridge (adr/0005-capture-encode-bridge-c-abi.md) ──────────── */

/* Poll one frame from `capture` (a live mediaway_camera_capture_t* opened via
 * device.h's mediaway_camera_capture_open) and push it into `session` — no
 * intermediate mediaway_camera_frame_t exposed, no extra copy. *out_wrote_frame ==
 * false is a valid "no new frame ready yet" result (the underlying poll returned
 * nothing new), not an error — mirrors mediaway_camera_capture_poll_frame's own
 * out_has_frame shape. Calls mediaway_camera_capture_release_frame internally after
 * the push attempt (documented no-op for Camera today, called anyway for contract
 * symmetry). Neither handle is closed or consumed by this function. */
mediaway_pipeline_status_t mediaway_encode_session_write_frame_from_camera_capture(
    mediaway_encode_session_t *session, mediaway_camera_capture_t *capture,
    bool *out_wrote_frame);

/* Same shape as mediaway_encode_session_write_frame_from_camera_capture, for a
 * mediaway_desktop_capture_t* (Screen) instead of Camera. GPU frames pass through
 * Zero-Copy: the polled frame's GPU handle moves straight into the encoder with no
 * CPU copy. mediaway_desktop_capture_release_frame is called internally after the
 * push attempt, success or failure — never left held into the next poll. */
mediaway_pipeline_status_t mediaway_encode_session_write_frame_from_desktop_capture(
    mediaway_encode_session_t *session, mediaway_desktop_capture_t *capture,
    bool *out_wrote_frame);

#ifdef __cplusplus
}
#endif

#endif /* MEDIAWAY_PIPELINE_H */
