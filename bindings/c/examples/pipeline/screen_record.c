/*
 * screen_record.c — screen + mic → encode → MP4.
 *
 * STATUS: NOT AVAILABLE FROM C — this file demonstrates the REAL gap. The
 * truth table in bindings/c/README.md marks screen_record "🚧 UNSUPPORTED from
 * C": Screen capture needs a live GPU device handle (ID3D11Device*) with no
 * CPU fallback (adr/0003-gpu-handle-c-abi.md), and there is no C representation
 * for the caller to construct or own one. Concretely, the real
 * mediaway-device-ffi ABI (post adr/0004-domain-feature-split) behaves like
 * this:
 *   - Screen config with the only C-constructible gpu_device
 *     (MEDIAWAY_GPU_DEVICE_NONE) -> MEDIAWAY_DEVICE_STATUS_INVALID_INPUT
 *     (a NONE handle is rejected, not silently ignored — adr/0003 §4);
 *   - Window-kind config -> MEDIAWAY_DEVICE_STATUS_UNSUPPORTED (no C
 *     constructor for a Window session; adr/0001 § Deferred).
 * We exercise BOTH, print the honest explanation, and exit gracefully. Any
 * other status means the ABI changed — we fail loudly instead of guessing.
 *
 * Build (see bindings/c/README.md "Building & verifying on Windows"):
 *   gcc -Icrates/mediaway-device-ffi/include bindings/c/examples/screen_record.c \
 *       -Ltarget/x86_64-pc-windows-gnu/debug -lmediaway_device_ffi -o screen_record.exe
 */

#include <mediaway/device.h>

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

int main(void) {
    if (mediaway_device_ffi_abi_version() != MEDIAWAY_DEVICE_FFI_ABI_VERSION) {
        fprintf(stderr,
                "ABI version mismatch: header %d, library %u — rebuild against "
                "the same mediaway-device-ffi\n",
                MEDIAWAY_DEVICE_FFI_ABI_VERSION,
                mediaway_device_ffi_abi_version());
        return EXIT_FAILURE;
    }

    const mediaway_rational_t time_base = {1, 30};
    const mediaway_gpu_device_handle_t no_gpu_device = {
        .kind = MEDIAWAY_GPU_DEVICE_NONE,
        .native = 0,
        .webgpu_device_id = 0,
    };

    /* ── 1. Screen + the only C-constructible gpu_device (NONE) ────────────
     * A real Screen session needs a live ID3D11Device* behind `native`; a C
     * caller has no way to build one, so this is the only value reachable
     * from C — and the ABI rejects it as INVALID_INPUT. */
    mediaway_desktop_capture_config_t screen_cfg =
        mediaway_desktop_capture_config_screen(/* output_index */ 0, time_base, no_gpu_device);
    mediaway_desktop_capture_t *capture = NULL;
    mediaway_device_status_t st = mediaway_desktop_capture_open(&screen_cfg, &capture);
    if (st != MEDIAWAY_DEVICE_STATUS_INVALID_INPUT) {
        fprintf(stderr,
                "FATAL: Screen + NONE gpu_device returned status %d, expected "
                "MEDIAWAY_DEVICE_STATUS_INVALID_INPUT. The ABI changed — screen "
                "capture from C may now be representable.\n",
                (int)st);
        if (st == MEDIAWAY_DEVICE_STATUS_OK && capture != NULL) {
            mediaway_desktop_capture_close(capture); /* don't leak a real handle */
        }
        return EXIT_FAILURE;
    }

    /* ── 2. Window-kind config — deterministically UNSUPPORTED (no C
     * constructor for Window this pass). */
    mediaway_desktop_capture_config_t window_cfg = screen_cfg;
    window_cfg.source_kind = MEDIAWAY_DESKTOP_CAPTURE_SOURCE_WINDOW;
    st = mediaway_desktop_capture_open(&window_cfg, &capture);
    if (st != MEDIAWAY_DEVICE_STATUS_UNSUPPORTED) {
        fprintf(stderr,
                "FATAL: Window-kind config returned status %d, expected "
                "MEDIAWAY_DEVICE_STATUS_UNSUPPORTED. The ABI changed.\n",
                (int)st);
        if (st == MEDIAWAY_DEVICE_STATUS_OK && capture != NULL) {
            mediaway_desktop_capture_close(capture);
        }
        return EXIT_FAILURE;
    }

    printf("Screen capture is NOT available from C in this pass:\n");
    printf("  it needs a live GPU device handle (ID3D11Device*) with no CPU\n");
    printf("  fallback, and a C caller cannot construct or own one yet —\n");
    printf("  Screen + NONE gpu_device -> INVALID_INPUT (status 5), Window ->\n");
    printf("  UNSUPPORTED (status 3). Both verified against the real ABI\n");
    printf("  (bindings/c/README.md truth table).\n");
    printf("Exiting gracefully — there is nothing to encode yet.\n");
    return EXIT_SUCCESS;
}
