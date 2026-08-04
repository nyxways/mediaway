# Python

Data-processing pipelines and ML input/output streams call Mediaway via `ctypes`/`cffi`
over the [`mediaway-ffi`](https://github.com/nyxways/mediaway/tree/main/crates/mediaway-ffi)
C ABI. Status: ✅ verified.

Examples live in [`bindings/python/examples/`](https://github.com/nyxways/mediaway/tree/main/bindings/python/examples):

| Capability | Example files |
|------------|---------------|
| Container | `container/mux_roundtrip.py` |
| Device | `device/camera_record.py` · `capture_microphone.py` · `capture_screen.py` |
| Pipeline | `pipeline/encode_audio.py` · `encode_to_mp4.py` · `screen_record.py` |

Build and run instructions: [`bindings/python/README.md`](https://github.com/nyxways/mediaway/blob/main/bindings/python/README.md).
