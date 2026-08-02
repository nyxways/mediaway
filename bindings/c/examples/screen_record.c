/*
 * screen_record.c — Mediaway screen capture C ABI reachability check.
 *
 * NOT a working screen recorder, and cannot become one from C in this pass: per
 * `crates/mediaway-device-ffi/adr/0001-capture-c-abi.md` § Finding 2 and §
 * Deferred, `WindowsScreenCapture` / `WindowsWindowCapture` are real,
 * hardware-verified, Zero-Copy Rust backends, but BOTH require a live
 * `GpuDeviceHandle::DirectX11(ID3D11Device*)` and reject `CpuFramesOk` outright —
 * there is no CPU fallback, and no C ABI representation for a GPU device handle
 * exists yet (the same open problem `mediaway-pipeline-ffi/adr/0001` already
 * deferred for `AutoVideoEncodeConfig::gpu_device`; see
 * `docs/spec/gpu-interop.md`). `mediaway_video_capture_open()` on a Screen-kind
 * config therefore ALWAYS returns `MEDIAWAY_DEVICE_STATUS_UNSUPPORTED` today —
 * real, deterministic, documented behavior (`include/mediaway/device.h`'s own
 * file header), not a bug this example works around.
 *
 * This file previously sketched a full screen+mic capture -> encode -> MP4 flow
 * (the same shape as the corrected `bindings/c/examples/camera_record.c`) before
 * the real ABI existed. That flow can never succeed from C until a follow-up ADR
 * designs a GPU-device-handle C representation, so it has been replaced with
 * exactly what the real ABI does today: open, observe UNSUPPORTED, and exit
 * gracefully with an explanation. This is the one part of this example that is
 * real, buildable, and link+run-verified in this dev environment (see
 * `docs/ai/wiki/device/ffi-c-abi.md`'s "Building the C examples on Windows"
 * section) — it demonstrates a real capability gap, not a working capture path.
 */

#include <mediaway/device.h>

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

int main(void) {
    const uint32_t fps = 30;
    const mediaway_rational_t tb = {.num = 1, .den = fps};

    /* Display index 0 = primary display. mediaway_video_capture_open() on a
     * Screen-kind config ALWAYS returns MEDIAWAY_DEVICE_STATUS_UNSUPPORTED in
     * this pass — see the file header. */
    mediaway_video_capture_config_t video_cfg = mediaway_video_capture_config_screen(0, tb);
    mediaway_video_capture_t *video = NULL;
    mediaway_device_status_t status = mediaway_video_capture_open(&video_cfg, &video);

    if (status != MEDIAWAY_DEVICE_STATUS_UNSUPPORTED) {
        /* A surprise: this file's whole premise (see the header comment) is that
         * screen capture cannot be opened from C in this pass. Any other status —
         * including MEDIAWAY_DEVICE_STATUS_OK — means the real ABI's behavior
         * changed and this example is out of date. */
        fprintf(stderr,
                "screen_record: unexpected status %d opening screen capture (expected "
                "MEDIAWAY_DEVICE_STATUS_UNSUPPORTED) — this example is out of date, see "
                "the file header\n",
                (int)status);
        if (video != NULL) {
            mediaway_video_capture_close(video);
        }
        return EXIT_FAILURE;
    }

    printf("screen_record: screen capture is UNSUPPORTED from C in this pass (status %d) — "
           "WindowsScreenCapture (and WindowsWindowCapture) require a live GPU device handle "
           "(GpuDeviceHandle::DirectX11) with no CPU fallback, and there is no C ABI "
           "representation for a GPU device handle yet. See "
           "crates/mediaway-device-ffi/adr/0001-capture-c-abi.md Finding 2 and Deferred. "
           "Nothing to record — exiting.\n",
           (int)status);
    return EXIT_SUCCESS;
}
