/*
 * encode_to_mp4.c — Mediaway auto video encode -> fragmented MP4.
 *
 * This now matches the real C ABI in `crates/mediaway-pipeline-ffi`
 * (`include/mediaway/pipeline.h`, `adr/0001-auto-encode-c-abi.md`) — the
 * crate is not yet built as a packaged/distributed release, so this is still
 * a documentation example rather than a shipped sample, but the function
 * names, struct layouts, and ownership rules below are the real ABI, not
 * aspirational guesses. It mirrors examples/encode_to_mp4.rs. Type-checked
 * against the real header (`gcc -fsyntax-only`) and, in this dev environment,
 * also built + linked against the `x86_64-pc-windows-gnu` target output and
 * run (see `adr/0001-auto-encode-c-abi.md` and the crate's `docs/roadmap.md`
 * for the verification recipe).
 *
 * v1's auto-encoder always resolves the CPU-upload encode path from C (no
 * `gpu_device`/`backend`/`max_path_class` fields yet — see
 * `adr/0001-auto-encode-c-abi.md` §1), so despite the "Zero-Copy GPU path
 * preferred" language in the wrapped Rust layer, nothing here can reach it.
 * The pipeline still picks the best available OS CPU-upload H.264 encoder
 * automatically and wires its output packets into a fragmented MP4 muxer for
 * you — the caller just pushes raw frames and gets complete MP4 bytes back
 * from finish().
 */

#include <mediaway/pipeline.h>

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Check a mediaway_pipeline_status_t after every call; no exceptions/panics
 * cross the C ABI, so this is the idiomatic error-handling shape for this
 * layer. */
#define CHECK(call)                                                          \
    do {                                                                     \
        mediaway_pipeline_status_t _st = (call);                            \
        if (_st != MEDIAWAY_PIPELINE_STATUS_OK) {                            \
            fprintf(stderr, "%s failed: status %d\n", #call, (int)_st);      \
            exit(EXIT_FAILURE);                                              \
        }                                                                    \
    } while (0)

int main(void) {
    const uint32_t width = 640;
    const uint32_t height = 480;
    const uint32_t fps = 30;
    const uint32_t seconds = 3;
    const uint32_t frame_count = fps * seconds; /* 90 frames */

    /* ── 1. Config: defaults for H.264 at this resolution/framerate, then
     * override bitrate ──────────────────────────────────────────────────── */
    mediaway_auto_video_encode_config_t config = mediaway_auto_video_encode_config_h264(
        width, height, (mediaway_rational_t){.num = 1, .den = fps});
    config.bitrate_bps = 2000000; /* 2 Mbps */

    /* ── 2. Open the auto encoder — "try the best available backend on this
     * platform, tell me if none exists here". Only NO_BACKEND (no encoder
     * compiled in at all on this platform) is an expected, graceful outcome:
     * print and exit cleanly, do not abort. Any other non-OK status —
     * including UNSUPPORTED, which means a backend exists but rejected this
     * specific config — is treated as a real failure via CHECK below, since
     * this example's config is a known-good default
     * (`mediaway_auto_video_encode_config_h264`) and should never be rejected
     * by a present backend. See `adr/0001-auto-encode-c-abi.md`'s note on
     * NO_BACKEND vs UNSUPPORTED not being equally graceful. ──────────────── */
    mediaway_auto_encoder_t *encoder = NULL;
    mediaway_pipeline_status_t open_status = mediaway_auto_encoder_open(&config, &encoder);
    if (open_status == MEDIAWAY_PIPELINE_STATUS_NO_BACKEND) {
        printf("encode_to_mp4: no auto encoder backend available on this platform "
               "(status %d) — nothing to do\n",
               (int)open_status);
        return EXIT_SUCCESS;
    }
    CHECK(open_status);

    printf("encode_to_mp4: running on this platform\n");

    /* ── 3. Wrap the encoder in an encode session. This consumes `encoder`
     * UNCONDITIONALLY — success or failure — so `encoder` must never be
     * closed separately after this call, regardless of the returned status.
     * ─────────────────────────────────────────────────────────────────────── */
    mediaway_encode_session_t *session = NULL;
    CHECK(mediaway_encode_session_open(encoder, &session));

    /* ── 4. Synthetic grey NV12 source: width*height Y bytes (128) followed by
     * width*height/2 interleaved UV bytes (128). Reused for every frame; the
     * library reads it synchronously within write_frame, so it only needs to
     * stay valid for the duration of that call. ────────────────────────────── */
    const size_t nv12_len = (size_t)width * height + (size_t)width * height / 2;
    uint8_t *nv12_frame = malloc(nv12_len);
    if (nv12_frame == NULL) {
        fprintf(stderr, "encode_to_mp4: out of memory allocating frame buffer\n");
        /* `session` is open but not yet finished — abandon it explicitly
         * rather than leaking the handle on this early-exit path. */
        mediaway_encode_session_close(session);
        return EXIT_FAILURE;
    }
    memset(nv12_frame, 128, nv12_len);

    for (uint32_t pts = 0; pts < frame_count; pts++) {
        mediaway_video_frame_t frame = {
            .pts = (int64_t)pts,
            .duration = 1,
            .width = width,
            .height = height,
            .pixel_format = MEDIAWAY_PIXEL_FORMAT_NV12,
            .raw_bytes = nv12_frame,
            .raw_bytes_len = nv12_len,
        };
        CHECK(mediaway_encode_session_write_frame(session, &frame));
    }

    free(nv12_frame);

    /* ── 5. Flush the encoder, finalize the muxer, get the complete MP4 file.
     * This consumes `session` UNCONDITIONALLY — do not close it separately,
     * regardless of the returned status. ─────────────────────────────────── */
    uint8_t *mp4_bytes = NULL;
    size_t mp4_len = 0;
    CHECK(mediaway_encode_session_finish(session, &mp4_bytes, &mp4_len));

    FILE *out = fopen("out.mp4", "wb");
    if (out == NULL) {
        fprintf(stderr, "encode_to_mp4: failed to open out.mp4 for writing\n");
        mediaway_pipeline_ffi_buffer_free(mp4_bytes, mp4_len);
        return EXIT_FAILURE;
    }
    fwrite(mp4_bytes, 1, mp4_len, out);
    fclose(out);
    mediaway_pipeline_ffi_buffer_free(mp4_bytes, mp4_len);

    printf("encode_to_mp4: %u frames -> out.mp4 (%zu bytes)\n", frame_count, mp4_len);
    return EXIT_SUCCESS;
}
