# C

C hosts link directly against the [`mediaway-ffi`](https://github.com/nyxways/mediaway/tree/main/crates/mediaway-ffi)
C ABI facade — one shared library, opaque handles, integer status codes, and
hand-written headers under `include/mediaway/`. Status: ✅ verified (real binding source
built and run against the native libraries).

Examples live in [`bindings/c/examples/`](https://github.com/nyxways/mediaway/tree/main/bindings/c/examples):

| Capability | Example files |
|------------|---------------|
| Container | `container/mux_roundtrip.c` |
| Device | `device/camera_record.c` · `capture_microphone.c` · `capture_screen.c` |
| Pipeline | `pipeline/encode_audio.c` · `encode_to_mp4.c` · `screen_record.c` |

Build and run instructions: [`bindings/c/README.md`](https://github.com/nyxways/mediaway/blob/main/bindings/c/README.md).
