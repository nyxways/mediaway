/*
 * stream_encode.c — auto H.264 encode streamed to a file incrementally.
 *
 * STATUS: REAL (link+run verified). Calls only the shipped mediaway-ffi ABI
 * (adr/pipeline/0007-stream-bytes-aac-decode-support-probe.md §1), exactly as
 * <mediaway/pipeline.h> declares it.
 *
 * encode_to_mp4.c holds the whole recording in RAM until finish(). This one
 * drains the session with mediaway_encode_session_poll_bytes after every frame
 * and appends each chunk to the output file, so memory is bounded by the poll
 * cadence, not by the recording's length. finish() then returns only the
 * bytes not yet polled — the tail — which is appended last.
 *
 * Usage: stream_encode [out.mp4]   (default: stream_out.mp4)
 * NO_BACKEND (no encoder compiled in) is an expected, graceful outcome.
 *
 * Build (see bindings/c/README.md "Building & verifying on Windows"):
 *   gcc -Icrates/mediaway-ffi/include bindings/c/examples/pipeline/stream_encode.c \
 *       -L<dir with mediaway_ffi.dll> -lmediaway_ffi -o stream_encode.exe
 */

#include <mediaway/pipeline.h>

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

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

#define FRAME_COUNT 150 /* 5 s at 1/30 s */
#define WIDTH 640
#define HEIGHT 480

/* Append a library-owned buffer to `out` and release it. NULL/0 (nothing ready) is a no-op. */
static size_t append_and_free(FILE *out, uint8_t *data, size_t len) {
    if (data != NULL && len > 0) {
        if (fwrite(data, 1, len, out) != len) {
            fprintf(stderr, "short write to the output file\n");
            exit(EXIT_FAILURE);
        }
    }
    mediaway_pipeline_ffi_buffer_free(data, len);
    return len;
}

int main(int argc, char **argv) {
    const char *path = argc > 1 ? argv[1] : "stream_out.mp4";

    if (mediaway_pipeline_ffi_abi_version() != MEDIAWAY_PIPELINE_FFI_ABI_VERSION) {
        fprintf(stderr, "ABI version mismatch: header %d, library %u\n",
                MEDIAWAY_PIPELINE_FFI_ABI_VERSION, mediaway_pipeline_ffi_abi_version());
        return EXIT_FAILURE;
    }

    const size_t frame_bytes = WIDTH * HEIGHT + 2 * (WIDTH / 2) * (HEIGHT / 2);
    uint8_t *grey_nv12 = (uint8_t *)malloc(frame_bytes);
    if (grey_nv12 == NULL) return EXIT_FAILURE;
    memset(grey_nv12, 0x80, frame_bytes);

    mediaway_auto_video_encode_config_t config =
        mediaway_auto_video_encode_config_h264(WIDTH, HEIGHT, (mediaway_rational_t){1, 30});
    mediaway_auto_encoder_t *encoder = NULL;
    mediaway_pipeline_status_t st = mediaway_auto_encoder_open(&config, &encoder);
    if (st == MEDIAWAY_PIPELINE_STATUS_NO_BACKEND) {
        printf("no encode backend compiled in (NO_BACKEND) — exiting gracefully\n");
        free(grey_nv12);
        return EXIT_SUCCESS;
    }
    CHECK(st);

    /* Consumes `encoder` unconditionally; never close it afterward. */
    mediaway_encode_session_t *session = NULL;
    CHECK(mediaway_encode_session_open(encoder, &session));

    FILE *out = fopen(path, "wb");
    if (out == NULL) {
        fprintf(stderr, "cannot open %s for writing\n", path);
        mediaway_encode_session_close(session);
        free(grey_nv12);
        return EXIT_FAILURE;
    }

    mediaway_video_frame_t frame = {
        .width = WIDTH,
        .height = HEIGHT,
        .pixel_format = MEDIAWAY_PIXEL_FORMAT_NV12,
        .storage_kind = MEDIAWAY_VIDEO_FRAME_STORAGE_CPU,
        .raw_bytes = grey_nv12,
        .raw_bytes_len = frame_bytes,
    };
    size_t streamed = 0;
    int chunks = 0;
    for (int i = 0; i < FRAME_COUNT; i++) {
        frame.pts = i;
        frame.duration = 1;
        CHECK(mediaway_encode_session_write_frame(session, &frame));

        /* Whatever fMP4 bytes are ready now — often none between fragments. */
        uint8_t *data = NULL;
        size_t len = 0;
        CHECK(mediaway_encode_session_poll_bytes(session, &data, &len));
        if (len > 0) chunks++;
        streamed += append_and_free(out, data, len);
    }

    /* finish() flushes and returns only what was NOT polled: the tail. It consumes
     * `session` unconditionally; never call mediaway_encode_session_close afterward. */
    uint8_t *tail = NULL;
    size_t tail_len = 0;
    CHECK(mediaway_encode_session_finish(session, &tail, &tail_len));
    append_and_free(out, tail, tail_len);
    fclose(out);
    free(grey_nv12);

    printf("streamed %d frame(s) into %s: %zu bytes in %d poll(s) + %zu-byte tail\n", FRAME_COUNT,
           path, streamed, chunks, tail_len);
    return EXIT_SUCCESS;
}
