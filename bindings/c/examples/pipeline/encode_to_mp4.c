/*
 * encode_to_mp4.c — auto H.264 encode of 90 synthetic NV12 frames → fMP4 bytes.
 *
 * STATUS: REAL (link+run verified). The scenario table in bindings/c/README.md
 * marks encode_to_mp4 "✅ link+run verified" — this file calls only the shipped
 * mediaway-ffi ABI, exactly as <mediaway/pipeline.h> declares it.
 * Nothing here is aspirational.
 *
 * Flow: build an H.264 640x480 @ 1/30 s config, open the best available
 * encoder, write 90 solid-grey NV12 frames (every byte 0x80), finish the
 * session to get the complete fragmented-MP4 byte stream, and print its size.
 * NO_BACKEND (no encoder compiled in) is an expected, graceful outcome, not a
 * bug.
 *
 * Build (see bindings/c/README.md "Building & verifying on Windows"):
 *   gcc -Icrates/mediaway-ffi/include bindings/c/examples/encode_to_mp4.c \
 *       -Ltarget/x86_64-pc-windows-gnu/debug -lmediaway_ffi -o encode_to_mp4.exe
 */

#include <mediaway/pipeline.h>

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* CHECK: a call that should only fail on a programming bug — print and abort.
 * NO_BACKEND is handled explicitly before each CHECK, never through here. */
#define CHECK(call)                                                               \
    do {                                                                          \
        mediaway_pipeline_status_t _st_ = (call);                                 \
        if (_st_ != MEDIAWAY_PIPELINE_STATUS_OK) {                                \
            fprintf(stderr,                                                       \
                    "CHECK failed: %s -> mediaway_pipeline_status_t %d (%s:%d)\n", \
                    #call, (int)_st_, __FILE__, __LINE__);                        \
            exit(EXIT_FAILURE);                                                   \
        }                                                                         \
    } while (0)

#define FRAME_COUNT 90 /* 90 frames at 1/30 s = 3 s of video */
#define WIDTH 640
#define HEIGHT 480

int main(void) {
    if (mediaway_pipeline_ffi_abi_version() != MEDIAWAY_PIPELINE_FFI_ABI_VERSION) {
        fprintf(stderr,
                "ABI version mismatch: header %d, library %u — rebuild against "
                "the same mediaway-ffi\n",
                MEDIAWAY_PIPELINE_FFI_ABI_VERSION,
                mediaway_pipeline_ffi_abi_version());
        return EXIT_FAILURE;
    }

    /* One NV12 frame: W*H luma bytes + (W/2)*(H/2)*2 interleaved chroma bytes. */
    const size_t frame_bytes = WIDTH * HEIGHT + 2 * (WIDTH / 2) * (HEIGHT / 2);
    uint8_t *grey_nv12 = (uint8_t *)malloc(frame_bytes);
    if (grey_nv12 == NULL) {
        fprintf(stderr, "out of memory allocating %zu frame bytes\n", frame_bytes);
        return EXIT_FAILURE;
    }
    memset(grey_nv12, 0x80, frame_bytes); /* flat mid-grey luma and chroma */

    /* The H.264 sugar constructor defaults pixel_format to NV12, bitrate to 0
     * (backend default) and gpu_device to NONE (CPU-only input path). */
    mediaway_rational_t time_base = {1, 30};
    mediaway_auto_video_encode_config_t config =
        mediaway_auto_video_encode_config_h264(WIDTH, HEIGHT, time_base);

    mediaway_auto_encoder_t *encoder = NULL;
    mediaway_pipeline_status_t st = mediaway_auto_encoder_open(&config, &encoder);
    if (st == MEDIAWAY_PIPELINE_STATUS_NO_BACKEND) {
        printf("no encode backend compiled in (NO_BACKEND) — exiting gracefully\n");
        free(grey_nv12);
        return EXIT_SUCCESS;
    }
    CHECK(st);
    if (encoder == NULL) {
        fprintf(stderr, "mediaway_auto_encoder_open returned OK with NULL encoder\n");
        free(grey_nv12);
        return EXIT_FAILURE;
    }

    /* Opening the session consumes `encoder` UNCONDITIONALLY (success or
     * failure) — never call mediaway_auto_encoder_close on it after this. */
    mediaway_encode_session_t *session = NULL;
    st = mediaway_encode_session_open(encoder, &session);
    if (st == MEDIAWAY_PIPELINE_STATUS_NO_BACKEND) {
        printf("no encode backend compiled in (NO_BACKEND at session open) — "
               "exiting gracefully\n");
        free(grey_nv12);
        return EXIT_SUCCESS;
    }
    CHECK(st);
    if (session == NULL) {
        fprintf(stderr, "mediaway_encode_session_open returned OK with NULL session\n");
        free(grey_nv12);
        return EXIT_FAILURE;
    }

    /* One reusable frame view: raw_bytes is a caller-owned borrow, valid for
     * the duration of each write_frame call only. */
    mediaway_video_frame_t frame = {
        .width = WIDTH,
        .height = HEIGHT,
        .pixel_format = MEDIAWAY_PIXEL_FORMAT_NV12,
        .storage_kind = MEDIAWAY_VIDEO_FRAME_STORAGE_CPU,
        .raw_bytes = grey_nv12,   /* borrowed input — never freed by the library */
        .raw_bytes_len = frame_bytes,
        /* gpu_buffer left zeroed: unused for CPU storage */
    };
    for (int i = 0; i < FRAME_COUNT; i++) {
        frame.pts = i;      /* one 1/30 s tick per frame */
        frame.duration = 1;
        CHECK(mediaway_encode_session_write_frame(session, &frame));
    }

    /* finish() consumes `session` UNCONDITIONALLY — never call
     * mediaway_encode_session_close on it after this. The returned buffer is
     * library-owned; release it with mediaway_pipeline_ffi_buffer_free. */
    uint8_t *out_data = NULL;
    size_t out_len = 0;
    st = mediaway_encode_session_finish(session, &out_data, &out_len);
    CHECK(st);
    if (out_len == 0 || out_data == NULL) {
        fprintf(stderr, "mediaway_encode_session_finish returned OK with empty output\n");
        free(grey_nv12);
        return EXIT_FAILURE;
    }

    printf("encoded %d NV12 frame(s) (%ux%u) into %zu bytes of fragmented MP4\n",
           FRAME_COUNT, WIDTH, HEIGHT, out_len);

    /* Owned output: released with mediaway_pipeline_ffi_buffer_free. */
    mediaway_pipeline_ffi_buffer_free(out_data, out_len);
    free(grey_nv12); /* caller-owned frame buffer */
    return EXIT_SUCCESS;
}
