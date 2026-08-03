/*
 * capture_screen.c — screen capture quick start.
 *
 * STATUS: NOT AVAILABLE FROM C — this file demonstrates the REAL gap, like
 * pipeline/screen_record.c but capture-only. Screen capture needs a live GPU
 * device handle (ID3D11Device*) with no CPU fallback
 * (crates/mediaway-ffi/adr/0003-gpu-handle-c-abi.md); a C caller
 * cannot construct or own one, so:
 *   - Screen config with the only C-constructible gpu_device (NONE)
 *     -> MEDIAWAY_DEVICE_STATUS_INVALID_INPUT (a NONE handle is rejected,
 *     not silently ignored — adr/0003 §4);
 *   - Window-kind config -> MEDIAWAY_DEVICE_STATUS_UNSUPPORTED (no C
 *     constructor; adr/0001 § Deferred).
 * We exercise BOTH and exit gracefully. Any other status means the ABI
 * changed — fail loudly.
 */

#include <mediaway/device.h>

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

int main(void) {
    const mediaway_rational_t time_base = {1, 30};
    const mediaway_gpu_device_handle_t no_gpu_device = {
        .kind = MEDIAWAY_GPU_DEVICE_NONE,
        .native = 0,
        .webgpu_device_id = 0,
    };

    mediaway_desktop_capture_config_t screen_cfg =
        mediaway_desktop_capture_config_screen(/* output_index */ 0, time_base, no_gpu_device);
    mediaway_desktop_capture_t *capture = NULL;
    mediaway_device_status_t st = mediaway_desktop_capture_open(&screen_cfg, &capture);
    if (st != MEDIAWAY_DEVICE_STATUS_INVALID_INPUT) {
        fprintf(stderr,
                "FATAL: Screen + NONE gpu_device returned status %d, expected "
                "MEDIAWAY_DEVICE_STATUS_INVALID_INPUT. The ABI changed.\n",
                (int)st);
        if (st == MEDIAWAY_DEVICE_STATUS_OK && capture != NULL) {
            mediaway_desktop_capture_close(capture);
        }
        return EXIT_FAILURE;
    }

    mediaway_desktop_capture_config_t window_cfg = screen_cfg;
    window_cfg.source_kind = MEDIAWAY_DESKTOP_CAPTURE_SOURCE_WINDOW;
    st = mediaway_desktop_capture_open(&window_cfg, &capture);
    if (st != MEDIAWAY_DEVICE_STATUS_UNSUPPORTED) {
        fprintf(stderr, "FATAL: Window-kind config returned status %d, expected UNSUPPORTED.\n", (int)st);
        if (st == MEDIAWAY_DEVICE_STATUS_OK && capture != NULL) {
            mediaway_desktop_capture_close(capture);
        }
        return EXIT_FAILURE;
    }

    printf("Screen capture is NOT available from C in this pass:\n");
    printf("  it needs a live GPU device handle (ID3D11Device*) with no CPU\n");
    printf("  fallback, and a C caller cannot construct or own one yet —\n");
    printf("  Screen + NONE gpu_device -> INVALID_INPUT (status 5), Window ->\n");
    printf("  UNSUPPORTED (status 3). Both verified against the real ABI.\n");
    printf("Exiting gracefully — nothing to capture yet.\n");
    return EXIT_SUCCESS;
}
