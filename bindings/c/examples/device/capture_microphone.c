/*
 * capture_microphone.c — microphone capture quick start (raw PCM).
 *
 * ✅ REAL — the microphone capture capability is implemented in the native C
 * ABI (`mediaway-device-ffi`, `mediaway_audio_capture_*`); this example runs
 * against it. Mirrors examples/device/capture_microphone.rs.
 *
 * Opens the default microphone, polls ~2 seconds of raw interleaved PCM
 * frames (f32le), prints the negotiated format, and closes. No encoding —
 * there is no audio encoder in the ABI; this is the capture-only quick start.
 * No mic on this machine → exit gracefully.
 *
 * Build (see bindings/c/README.md "Building & verifying on Windows"):
 *   gcc -Icrates/mediaway-device-ffi/include bindings/c/examples/device/capture_microphone.c \
 *       -Ltarget/x86_64-pc-windows-gnu/debug -lmediaway_device_ffi -o capture_microphone.exe
 */

#include <mediaway/device.h>

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

#define CAPTURE_SECONDS 2
#define POLL_INTERVAL_MS 5

int main(void) {
    /* ── 1. Open the default microphone at 48 kHz (F32 — the format the real
     * Windows WASAPI backend delivers). ─────────────────────────────────── */
    const mediaway_rational_t tb = {1, 48000};
    mediaway_audio_capture_config_t config = mediaway_audio_capture_config_microphone(tb);
    mediaway_audio_capture_t *mic = NULL;
    mediaway_device_status_t st = mediaway_audio_capture_open(&config, &mic);
    if (st != MEDIAWAY_DEVICE_STATUS_OK) {
        printf("capture_microphone: no microphone available (status %d) — nothing to do\n", (int)st);
        return EXIT_SUCCESS; /* expected outcome, not a bug */
    }

    /* ── 2. Query the format the backend actually negotiated. ───────────── */
    uint32_t sample_rate = 0;
    uint16_t channels = 0;
    st = mediaway_audio_capture_format(mic, &sample_rate, &channels);
    if (st != MEDIAWAY_DEVICE_STATUS_OK) {
        fprintf(stderr, "capture_microphone: format query failed (status %d)\n", (int)st);
        mediaway_audio_capture_close(mic);
        return EXIT_FAILURE;
    }
    printf("capture_microphone: mic ready — %u Hz, %u ch\n", sample_rate, channels);

    /* ── 3. Poll PCM frames for CAPTURE_SECONDS. Frames are OWNED outputs:
     * each polled frame must be released with mediaway_audio_frame_free. ── */
    const clock_t start = clock();
    uint64_t frames = 0;
    uint64_t total_bytes = 0;
    while ((double)(clock() - start) / CLOCKS_PER_SEC < CAPTURE_SECONDS) {
        mediaway_device_audio_frame_t frame;
        bool has_frame = false;
        st = mediaway_audio_capture_poll_frame(mic, &frame, &has_frame);
        if (st != MEDIAWAY_DEVICE_STATUS_OK) {
            fprintf(stderr, "capture_microphone: poll failed (status %d)\n", (int)st);
            break;
        }
        if (has_frame) {
            total_bytes += frame.data_len;
            frames++;
            mediaway_audio_frame_free(&frame); /* owned output — released here */
        } else {
            /* Sleep a little between polls — no busy loop. */
            for (volatile int i = 0; i < POLL_INTERVAL_MS * 100000; i++) {
            }
        }
    }

    printf("capture_microphone: captured %llu frame(s), %llu bytes of PCM in %d s\n",
           (unsigned long long)frames, (unsigned long long)total_bytes, CAPTURE_SECONDS);

    /* ── 4. Close. BLOCKS up to one period interval (joins the backend
     * worker thread) — a real cost, not a pointer free. ─────────────────── */
    mediaway_audio_capture_close(mic);
    return EXIT_SUCCESS;
}
