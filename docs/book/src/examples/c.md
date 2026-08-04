# C

C hosts link directly against the [`mediaway-ffi`](https://github.com/nyxways/mediaway/tree/main/crates/mediaway-ffi)
C ABI facade — one shared library, opaque handles, integer status codes, and
hand-written headers under `include/mediaway/`. Status: ✅ verified (real binding source
built and run against the native libraries).

## Build and minimal usage

```bash
# Windows example from the examples (link against the built mediaway_ffi)
gcc -Icrates/mediaway-ffi/include bindings/c/examples/container/mux_roundtrip.c \
    -Ltarget/x86_64-pc-windows-gnu/debug -lmediaway_ffi -o mux_roundtrip.exe
```

```c
#include <mediaway/container.h>

mediaway_muxer_t *muxer = mediaway_muxer_create();
mediaway_video_track_info_t track = {
    .id = 0, .codec = MEDIAWAY_CODEC_H264,
    .time_base = { 1, 30 }, .width = 1920, .height = 1080,
};
mediaway_muxer_add_video_track(muxer, &track);
mediaway_muxer_begin(muxer);
/* push packets, flush, drain with mediaway_muxer_poll_bytes */
mediaway_muxer_close(muxer);
```

Examples live in [`bindings/c/examples/`](https://github.com/nyxways/mediaway/tree/main/bindings/c/examples):

| Capability | Example files |
|------------|---------------|
| Container | `container/mux_roundtrip.c` |
| Device | `device/camera_record.c` · `capture_microphone.c` · `capture_screen.c` |
| Pipeline | `pipeline/encode_audio.c` · `encode_to_mp4.c` · `screen_record.c` |

Build and run instructions: [`bindings/c/README.md`](https://github.com/nyxways/mediaway/blob/main/bindings/c/README.md).
