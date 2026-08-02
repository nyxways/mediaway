/*
 * camera_record.c — Mediaway camera + mic capture -> encode -> fragmented MP4.
 *
 * This now matches the real C ABI in `crates/mediaway-device-ffi`
 * (`include/mediaway/device.h`, `adr/0001-capture-c-abi.md`) for capture, and the
 * already-corrected real C ABI in `crates/mediaway-pipeline-ffi`
 * (`include/mediaway/pipeline.h`, `adr/0001-auto-encode-c-abi.md`) for encode.
 * Neither crate is yet built as a packaged/distributed release, so this is still
 * a documentation example rather than a shipped sample, but the function names,
 * struct layouts, and ownership rules below are the real ABI, not aspirational
 * guesses. Camera capture and microphone capture are both real, hardware-verified
 * Windows backends (`adr/0001-capture-c-abi.md` § Finding 1) — built + linked
 * against the `x86_64-pc-windows-gnu` target output and run in this dev
 * environment, actually opening a physical camera and capturing real frames (see
 * `docs/ai/wiki/device/ffi-c-abi.md`'s "Building the C examples on Windows"
 * section for the verification recipe and observed output).
 *
 * Corrected against the aspirational sketch per `adr/0001-capture-c-abi.md` §
 * Corrections:
 *   (a)/(c) mediaway_video_capture_poll_frame / mediaway_audio_capture_poll_frame
 *       combine status + owned frame + has-frame flag into ONE call — the same
 *       shape as mediaway_demuxer_poll_packet — instead of a separate "is a frame
 *       ready" + "fetch it" split.
 *   (b)/(e) mediaway_rational_t literals use {.num = ..., .den = ...} designated
 *       initializers, never (int32_t)/(uint32_t) casts.
 *   (d) mediaway_audio_capture_format queries the mic's real negotiated sample
 *       rate/channel count instead of assuming one.
 *   Every frame obtained via poll_frame is released with
 *       mediaway_device_video_frame_free / mediaway_device_audio_frame_free.
 *
 * mediaway_video_capture_close / mediaway_audio_capture_close now return a
 * mediaway_device_status_t (not void) and BLOCK for up to one frame/period
 * interval each — they join the backend's worker thread, a real,
 * non-instantaneous cost, not merely a pointer free (`include/mediaway/device.h`'s
 * own file header; `adr/0001-capture-c-abi.md` §9).
 *
 * NEW FINDING from getting this file to actually compile: <mediaway/pipeline.h>
 * independently (re)defines `mediaway_rational_t`/`mediaway_pixel_format_t`
 * identically to <mediaway/device.h> — both wrap the same shared
 * `mediaway_common::Rational`/`PixelFormat` end-to-end (each crate's own ADR §7
 * "reuse" table) — but #including both headers together does NOT compile: GCC
 * (verified here) rejects redefining a `struct`/`enum` tag with a body a second
 * time in one translation unit, even with byte-identical members. This is a real
 * C-level instance of the hazard `adr/0001-capture-c-abi.md` §7's "Open risk"
 * paragraph only flagged for C++ — it turns out plain C rejects it too, the
 * moment an example (like this one) needs both a capture header and an encode
 * header at once. Until a shared `mediaway-common-ffi` header exists (both
 * ADRs' § Deferred), this file only `#include`s <mediaway/device.h> and
 * hand-declares the small `mediaway-pipeline-ffi` surface it needs below,
 * reusing device.h's own `mediaway_rational_t`/`mediaway_pixel_format_t` —
 * binary-compatible with pipeline.h's definitions since both are `#[repr(C)]`
 * mirrors of the identical Rust types.
 */

#include <mediaway/device.h>

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

/* ── Hand-declared mediaway-pipeline-ffi surface (see the file header's "NEW
 * FINDING" paragraph for why <mediaway/pipeline.h> is not #included directly).
 * Copied verbatim from crates/mediaway-pipeline-ffi/include/mediaway/pipeline.h,
 * minus the mediaway_rational_t/mediaway_pixel_format_t definitions already
 * provided by <mediaway/device.h> above. ─────────────────────────────────────── */

typedef struct mediaway_auto_encoder mediaway_auto_encoder_t;
typedef struct mediaway_encode_session mediaway_encode_session_t;

typedef enum mediaway_pipeline_status {
    MEDIAWAY_PIPELINE_STATUS_OK                      = 0,
    MEDIAWAY_PIPELINE_STATUS_INVALID_ARGUMENT        = 1,
    MEDIAWAY_PIPELINE_STATUS_HANDLE_POISONED         = 2,
    MEDIAWAY_PIPELINE_STATUS_NO_BACKEND              = 3,
    MEDIAWAY_PIPELINE_STATUS_UNSUPPORTED             = 4,
    MEDIAWAY_PIPELINE_STATUS_INVALID_INPUT           = 5,
    MEDIAWAY_PIPELINE_STATUS_ENCODER_BACKEND_FAILURE = 6,
    MEDIAWAY_PIPELINE_STATUS_ENCODER_CLOSED          = 7,
    MEDIAWAY_PIPELINE_STATUS_MUX_INVALID_TRACK       = 8,
    MEDIAWAY_PIPELINE_STATUS_MUX_INVALID_PACKET      = 9,
    MEDIAWAY_PIPELINE_STATUS_MUX_INVALID_DATA        = 10,
    MEDIAWAY_PIPELINE_STATUS_UNKNOWN_ERROR           = 11,
    MEDIAWAY_PIPELINE_STATUS_INTERNAL_PANIC          = 12,
} mediaway_pipeline_status_t;

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
} mediaway_pipeline_codec_kind_t;

typedef struct mediaway_auto_video_encode_config {
    mediaway_pipeline_codec_kind_t codec;
    uint32_t width;
    uint32_t height;
    mediaway_rational_t time_base;
    uint32_t bitrate_bps;
    mediaway_pixel_format_t pixel_format;
} mediaway_auto_video_encode_config_t;

/* Borrowed input to mediaway_encode_session_write_frame — distinct from
 * mediaway_device_video_frame_t (poll_frame's owned output, see device.h). */
typedef struct mediaway_video_frame {
    int64_t pts;
    uint64_t duration;
    uint32_t width;
    uint32_t height;
    mediaway_pixel_format_t pixel_format;
    const uint8_t *raw_bytes;
    size_t raw_bytes_len;
} mediaway_video_frame_t;

mediaway_auto_video_encode_config_t mediaway_auto_video_encode_config_h264(
    uint32_t width, uint32_t height, mediaway_rational_t time_base);
mediaway_pipeline_status_t mediaway_auto_encoder_open(
    const mediaway_auto_video_encode_config_t *config, mediaway_auto_encoder_t **out_encoder);
mediaway_pipeline_status_t mediaway_encode_session_open(
    mediaway_auto_encoder_t *encoder, mediaway_encode_session_t **out_session);
mediaway_pipeline_status_t mediaway_encode_session_write_frame(
    mediaway_encode_session_t *session, const mediaway_video_frame_t *frame);
mediaway_pipeline_status_t mediaway_encode_session_finish(
    mediaway_encode_session_t *session, uint8_t **out_data, size_t *out_len);
void mediaway_pipeline_ffi_buffer_free(uint8_t *data, size_t len);

/* Check a mediaway_device_status_t after every capture call; no exceptions/panics
 * cross the C ABI, so this is the idiomatic error-handling shape for this layer.
 * Only used where failure means a programming bug, not an expected runtime
 * condition (missing hardware, unavailable backend) — those are handled inline
 * instead, see main() below. */
#define CHECK_DEVICE(call)                                                  \
    do {                                                                    \
        mediaway_device_status_t _st = (call);                              \
        if (_st != MEDIAWAY_DEVICE_STATUS_OK) {                             \
            fprintf(stderr, "%s failed: status %d\n", #call, (int)_st);     \
            exit(EXIT_FAILURE);                                             \
        }                                                                   \
    } while (0)

/* Check a mediaway_pipeline_status_t after every encode call; same reasoning as
 * CHECK_DEVICE above, for the sibling status enum. */
#define CHECK_PIPELINE(call)                                                \
    do {                                                                    \
        mediaway_pipeline_status_t _st = (call);                           \
        if (_st != MEDIAWAY_PIPELINE_STATUS_OK) {                           \
            fprintf(stderr, "%s failed: status %d\n", #call, (int)_st);     \
            exit(EXIT_FAILURE);                                             \
        }                                                                   \
    } while (0)

/*
 * record() — poll real video frames from `video` and real PCM frames from `mic`
 * (mic may be NULL: recording continues without audio), writing the camera's own
 * captured bytes into `session` for each video frame, until `duration_seconds`
 * elapses.
 *
 * This function only sees opaque handles: it does not know or care which
 * concrete OS backend produced them (Media Foundation, V4L2, ...) — that
 * dispatch already happened by the time `video`/`mic` were opened by the caller.
 */
static void record(mediaway_video_capture_t *video, mediaway_audio_capture_t *mic,
                    mediaway_encode_session_t *session, double duration_seconds) {
    const clock_t start = clock();

    while ((double)(clock() - start) / CLOCKS_PER_SEC < duration_seconds) {
        /* ── Video: poll returns status + owned frame + has-frame flag in one call.
         * *has_video_frame == false is a valid "no frame yet" result, not an
         * error. ─────────────────────────────────────────────────────────────── */
        mediaway_device_video_frame_t video_frame;
        bool has_video_frame = false;
        mediaway_device_status_t video_poll_status =
            mediaway_video_capture_poll_frame(video, &video_frame, &has_video_frame);
        if (video_poll_status != MEDIAWAY_DEVICE_STATUS_OK) {
            fprintf(stderr, "camera_record: video capture poll error: status %d\n",
                    (int)video_poll_status);
            return;
        }
        if (has_video_frame) {
            /* Write the camera's real captured bytes straight into the encoder —
             * no synthetic placeholder needed. */
            mediaway_video_frame_t frame = {
                .pts = video_frame.pts,
                .duration = video_frame.duration,
                .width = video_frame.width,
                .height = video_frame.height,
                .pixel_format = video_frame.pixel_format,
                .raw_bytes = video_frame.data,
                .raw_bytes_len = video_frame.data_len,
            };
            CHECK_PIPELINE(mediaway_encode_session_write_frame(session, &frame));
            /* Release the FFI-owned copy, then tell the backend the last polled
             * frame's resources may be reused (documented no-op for Camera today,
             * but the trait contract still requires it before the next
             * frame-acquiring poll). */
            mediaway_device_video_frame_free(&video_frame);
            CHECK_DEVICE(mediaway_video_capture_release_frame(video));
        }

        /* ── Audio: drain whatever is pending, freeing every owned frame. Not
         * wired into the encode session yet (no audio track/encoder in this
         * example) — just keep the capture queue from backing up. ───────────── */
        if (mic != NULL) {
            for (;;) {
                mediaway_device_audio_frame_t audio_frame;
                bool has_audio_frame = false;
                mediaway_device_status_t audio_poll_status =
                    mediaway_audio_capture_poll_frame(mic, &audio_frame, &has_audio_frame);
                if (audio_poll_status != MEDIAWAY_DEVICE_STATUS_OK || !has_audio_frame) {
                    break;
                }
                /* TODO(#issue): push to an audio encoder / second track. */
                mediaway_device_audio_frame_free(&audio_frame);
            }
        }
    }
}

int main(void) {
    const uint32_t fps = 30;
    const mediaway_rational_t tb = {.num = 1, .den = fps};

    /* ── 1. Open camera capture. Opening is fallible — the OS/platform backend or
     * this specific camera device may not be available; handle that gracefully
     * instead of crashing. Device index 0 = default/first camera. ────────────── */
    mediaway_video_capture_config_t video_cfg = mediaway_video_capture_config_camera(0, tb);
    mediaway_video_capture_t *video = NULL;
    mediaway_device_status_t video_status = mediaway_video_capture_open(&video_cfg, &video);
    if (video_status != MEDIAWAY_DEVICE_STATUS_OK) {
        printf("camera_record: camera unavailable on this platform "
               "(status %d) — nothing to do\n",
               (int)video_status);
        return EXIT_SUCCESS;
    }

    /* ── 2. Open the microphone. Also fallible; a missing mic should not stop
     * recording — continue video-only. ───────────────────────────────────────── */
    mediaway_audio_capture_config_t mic_cfg =
        mediaway_audio_capture_config_microphone((mediaway_rational_t){.num = 1, .den = 48000});
    mediaway_audio_capture_t *mic = NULL;
    mediaway_device_status_t mic_status = mediaway_audio_capture_open(&mic_cfg, &mic);
    if (mic_status != MEDIAWAY_DEVICE_STATUS_OK) {
        printf("camera_record: microphone unavailable (status %d) — "
               "continuing without audio\n",
               (int)mic_status);
        mic = NULL;
    }

    /* ── 3. Query the stream geometry the camera actually negotiated — do not
     * assume a resolution. ────────────────────────────────────────────────────── */
    uint32_t width = 0;
    uint32_t height = 0;
    CHECK_DEVICE(mediaway_video_capture_geometry(video, &width, &height));

    /* ── 3b. Query the mic's real negotiated sample rate/channel count — do not
     * assume a format, only meaningful if the mic actually opened. ───────────── */
    uint32_t mic_sample_rate = 0;
    uint16_t mic_channels = 0;
    if (mic != NULL) {
        CHECK_DEVICE(mediaway_audio_capture_format(mic, &mic_sample_rate, &mic_channels));
        printf("camera_record: %ux%u camera, mic ready (%u Hz, %u ch)\n", width, height,
               mic_sample_rate, mic_channels);
    } else {
        printf("camera_record: %ux%u camera, mic unavailable\n", width, height);
    }

    /* ── 4. Config: defaults for H.264 at the capture's real resolution and frame
     * rate, then override bitrate — same shape as the encode-only quick start
     * (bindings/c/encode_to_mp4.c). ───────────────────────────────────────────── */
    mediaway_auto_video_encode_config_t enc_cfg =
        mediaway_auto_video_encode_config_h264(width, height, tb);
    enc_cfg.bitrate_bps = 4000000; /* 4 Mbps */

    mediaway_auto_encoder_t *encoder = NULL;
    mediaway_pipeline_status_t enc_status = mediaway_auto_encoder_open(&enc_cfg, &encoder);
    if (enc_status != MEDIAWAY_PIPELINE_STATUS_OK) {
        printf("camera_record: no auto encoder backend available on this platform "
               "(status %d) — nothing to do\n",
               (int)enc_status);
        CHECK_DEVICE(mediaway_video_capture_close(video));
        if (mic != NULL) {
            CHECK_DEVICE(mediaway_audio_capture_close(mic));
        }
        return EXIT_SUCCESS;
    }

    /* Wrap the encoder in an encode session. On success this consumes `encoder` —
     * do not close it separately. */
    mediaway_encode_session_t *session = NULL;
    CHECK_PIPELINE(mediaway_encode_session_open(encoder, &session));

    /* ── 5. Record: one small, reusable function that only ever sees opaque
     * handles — no OS-specific code below this line. ─────────────────────────── */
    record(video, mic, session, 3.0 /* seconds */);

    /* ── 6. Close capture handles once recording is done. Each BLOCKS for up to
     * one frame/period interval — joins the backend's worker thread, a real,
     * non-instantaneous cost, not merely a pointer free. ──────────────────────── */
    CHECK_DEVICE(mediaway_video_capture_close(video));
    if (mic != NULL) {
        CHECK_DEVICE(mediaway_audio_capture_close(mic));
    }

    /* ── 7. Flush the encoder, finalize the muxer, get the complete MP4 file.
     * This consumes `session` — do not close it separately. ──────────────────── */
    uint8_t *mp4_bytes = NULL;
    size_t mp4_len = 0;
    CHECK_PIPELINE(mediaway_encode_session_finish(session, &mp4_bytes, &mp4_len));

    FILE *out = fopen("out_camera.mp4", "wb");
    if (out == NULL) {
        fprintf(stderr, "camera_record: failed to open out_camera.mp4 for writing\n");
        mediaway_pipeline_ffi_buffer_free(mp4_bytes, mp4_len);
        return EXIT_FAILURE;
    }
    fwrite(mp4_bytes, 1, mp4_len, out);
    fclose(out);
    mediaway_pipeline_ffi_buffer_free(mp4_bytes, mp4_len);

    printf("camera_record: -> out_camera.mp4 (%zu bytes)\n", mp4_len);
    return EXIT_SUCCESS;
}
