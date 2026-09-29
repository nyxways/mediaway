/*
 * capture_window.c — single-window capture quick start.
 *
 * STATUS: REAL (link+run verified on Windows). Window capture is WGC by HWND
 * (crates/mediaway-ffi/adr/device/0005-window-capture-c-abi.md). Like Screen it
 * has no CPU fallback: it needs a live GPU device from mediaway_gpu_device_create().
 *
 * Usage:
 *   capture_window [hwnd-in-hex]
 * With no argument the current foreground window is captured. The HWND is
 * caller-owned and must stay valid for the whole session.
 *
 * The config below exercises the ADR-0005 options: the pointer is excluded (the
 * default), the OS capture border is requested hidden and READ BACK with
 * mediaway_desktop_capture_border_hidden (a refusal is not an error at open),
 * and frames are even-cropped so any hardware encoder accepts them. Set
 * cfg.region_enabled and cfg.region_* to record a rectangle instead; a region
 * away from the window's origin costs one GPU copy per frame (not Zero-Copy).
 */

#include <mediaway/device.h>

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <windows.h> /* GetForegroundWindow, Sleep — this example is Windows-only */

#define POLL_FRAMES 5
#define POLL_TIMEOUT_MS 5000
#define POLL_INTERVAL_MS 20

int main(int argc, char **argv) {
    uint64_t hwnd = 0;
    if (argc > 1) {
        hwnd = (uint64_t)strtoull(argv[1], NULL, 16);
    } else {
        hwnd = (uint64_t)(uintptr_t)GetForegroundWindow();
    }
    if (hwnd == 0) {
        printf("no window to capture (pass an HWND in hex) — exiting gracefully\n");
        return EXIT_SUCCESS;
    }

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

    mediaway_desktop_capture_config_t cfg =
        mediaway_desktop_capture_config_window(hwnd, (mediaway_rational_t){1, 30}, gpu_handle);
    cfg.cursor = MEDIAWAY_CAPTURE_CURSOR_EXCLUDED;
    cfg.border = MEDIAWAY_CAPTURE_BORDER_HIDDEN;
    cfg.dimensions = MEDIAWAY_FRAME_DIMENSIONS_EVEN_CROPPED;

    mediaway_desktop_capture_t *capture = NULL;
    st = mediaway_desktop_capture_open(&cfg, &capture);
    if (st == MEDIAWAY_DEVICE_STATUS_REGION_OUT_OF_BOUNDS) {
        fprintf(stderr, "FATAL: the region does not fit the window\n");
        mediaway_gpu_device_close(gpu_device);
        return EXIT_FAILURE;
    }
    if (st != MEDIAWAY_DEVICE_STATUS_OK) {
        printf("no window capture available (mediaway_desktop_capture_open -> "
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
    bool border_hidden = false;
    st = mediaway_desktop_capture_border_hidden(capture, &border_hidden);
    if (st != MEDIAWAY_DEVICE_STATUS_OK) {
        fprintf(stderr, "FATAL: mediaway_desktop_capture_border_hidden failed (status %d)\n",
                (int)st);
        mediaway_desktop_capture_close(capture);
        mediaway_gpu_device_close(gpu_device);
        return EXIT_FAILURE;
    }
    printf("window %llx negotiated %ux%u, border hidden by the OS: %s\n",
           (unsigned long long)hwnd, width, height, border_hidden ? "yes" : "no");

    int polled = 0;
    int waited_ms = 0;
    mediaway_desktop_frame_t frame;
    while (polled < POLL_FRAMES && waited_ms < POLL_TIMEOUT_MS) {
        bool has_frame = false;
        st = mediaway_desktop_capture_poll_frame(capture, &frame, &has_frame);
        if (st != MEDIAWAY_DEVICE_STATUS_OK) {
            fprintf(stderr, "FATAL: mediaway_desktop_capture_poll_frame failed (status %d)\n",
                    (int)st);
            mediaway_desktop_capture_close(capture);
            mediaway_gpu_device_close(gpu_device);
            return EXIT_FAILURE;
        }
        if (has_frame) {
            printf("polled frame %d: pts=%lld %ux%u storage_kind=%d\n", polled + 1,
                   (long long)frame.pts, frame.width, frame.height, (int)frame.storage_kind);
            /* GPU storage: gpu_buffer is BORROWED, never freed — release the slot. */
            mediaway_desktop_capture_release_frame(capture);
            polled++;
            waited_ms = 0;
        } else {
            Sleep(POLL_INTERVAL_MS);
            waited_ms += POLL_INTERVAL_MS;
        }
    }
    printf("polled %d real window frame(s)\n", polled);

    /* Closing joins the backend's worker thread: can block for up to one
     * frame/period interval (documented cost, not just a pointer free). */
    mediaway_desktop_capture_close(capture);
    mediaway_gpu_device_close(gpu_device);
    return EXIT_SUCCESS;
}
