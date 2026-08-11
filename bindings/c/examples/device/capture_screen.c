/*
 * capture_screen.c — screen capture quick start.
 *
 * STATUS: REAL (link+run verified) as of the GPU device factory
 * (mediaway-device ADR-0007). Before it, a plain C caller had no way to
 * construct or own a live GPU device handle (ID3D11Device*) — Screen capture
 * has no CPU fallback (crates/mediaway-ffi/adr/0003-gpu-handle-c-abi.md), so
 * every Screen config was unreachable from C. `mediaway_gpu_device_create()`
 * closes that gap: this example creates a real device (default adapter) and
 * drives real Screen capture with it. Window-kind capture is still
 * deliberately deferred (needs a native HWND input, adr/0001 § Deferred) —
 * that path still returns MEDIAWAY_DEVICE_STATUS_UNSUPPORTED and this example
 * still exercises it.
 */

#include <mediaway/device.h>

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <windows.h> /* Sleep — this example is Windows-only (Screen capture) */

#define POLL_FRAMES 5
#define POLL_TIMEOUT_MS 5000
#define POLL_INTERVAL_MS 20

int main(void) {
    mediaway_gpu_device_options_t gpu_options = {
        .adapter = {.kind = MEDIAWAY_GPU_ADAPTER_SELECT_DEFAULT, .index = 0},
        .video_support = true,
        .debug_layer = false,
    };
    mediaway_gpu_device_t *gpu_device = NULL;
    mediaway_device_status_t st = mediaway_gpu_device_create(&gpu_options, &gpu_device);
    if (st != MEDIAWAY_DEVICE_STATUS_OK) {
        printf("no usable GPU device (mediaway_gpu_device_create -> status %d) — "
               "exiting gracefully\n",
               (int)st);
        return EXIT_SUCCESS;
    }
    mediaway_gpu_device_handle_t gpu_handle;
    st = mediaway_gpu_device_handle(gpu_device, &gpu_handle);
    if (st != MEDIAWAY_DEVICE_STATUS_OK) {
        fprintf(stderr, "FATAL: mediaway_gpu_device_handle failed (status %d)\n", (int)st);
        mediaway_gpu_device_close(gpu_device);
        return EXIT_FAILURE;
    }

    const mediaway_rational_t time_base = {1, 30};
    mediaway_desktop_capture_config_t screen_cfg =
        mediaway_desktop_capture_config_screen(/* output_index */ 0, time_base, gpu_handle);
    mediaway_desktop_capture_t *capture = NULL;
    st = mediaway_desktop_capture_open(&screen_cfg, &capture);
    if (st != MEDIAWAY_DEVICE_STATUS_OK) {
        printf("no screen capture available (mediaway_desktop_capture_open -> "
               "status %d) — exiting gracefully\n",
               (int)st);
        mediaway_gpu_device_close(gpu_device);
        return EXIT_SUCCESS;
    }

    uint32_t width = 0;
    uint32_t height = 0;
    st = mediaway_desktop_capture_geometry(capture, &width, &height);
    if (st != MEDIAWAY_DEVICE_STATUS_OK) {
        fprintf(stderr, "FATAL: mediaway_desktop_capture_geometry failed (status %d)\n", (int)st);
        mediaway_desktop_capture_close(capture);
        mediaway_gpu_device_close(gpu_device);
        return EXIT_FAILURE;
    }
    printf("screen negotiated %ux%u\n", width, height);

    int polled = 0;
    int waited_ms = 0;
    mediaway_desktop_frame_t frame;
    while (polled < POLL_FRAMES && waited_ms < POLL_TIMEOUT_MS) {
        bool has_frame = false;
        st = mediaway_desktop_capture_poll_frame(capture, &frame, &has_frame);
        if (st != MEDIAWAY_DEVICE_STATUS_OK) {
            fprintf(stderr, "FATAL: mediaway_desktop_capture_poll_frame failed (status %d)\n", (int)st);
            mediaway_desktop_capture_close(capture);
            mediaway_gpu_device_close(gpu_device);
            return EXIT_FAILURE;
        }
        if (has_frame) {
            printf("polled frame %d: pts=%lld %ux%u storage_kind=%d\n", polled + 1,
                   (long long)frame.pts, frame.width, frame.height, (int)frame.storage_kind);
            /* GPU storage (the only kind Screen capture produces): gpu_buffer
             * is BORROWED, never freed — release the slot instead. */
            mediaway_desktop_capture_release_frame(capture);
            polled++;
            waited_ms = 0;
        } else {
            Sleep(POLL_INTERVAL_MS);
            waited_ms += POLL_INTERVAL_MS;
        }
    }
    printf("polled %d real screen frame(s)\n", polled);

    /* Closing joins the backend's worker thread: can block for up to one
     * frame/period interval (documented cost, not just a pointer free). */
    mediaway_desktop_capture_close(capture);
    mediaway_gpu_device_close(gpu_device);
    return EXIT_SUCCESS;
}
