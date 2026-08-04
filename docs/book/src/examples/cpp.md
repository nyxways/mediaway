# C++

A thin RAII wrapper over the [`mediaway-ffi`](https://github.com/nyxways/mediaway/tree/main/crates/mediaway-ffi)
C ABI — for native desktop apps, custom engines, and rendering pipelines. Status:
✅ verified.

> **Note:** the C++ RAII wrapper surface is at design stage — the examples target it
> (nothing compiles yet). The underlying `mediaway-ffi` C ABI is real and link-run
> verified; C++ apps can call it directly today. Build with CMake per
> [`bindings/cpp/README.md`](https://github.com/nyxways/mediaway/blob/main/bindings/cpp/README.md).

Examples live in [`bindings/cpp/examples/`](https://github.com/nyxways/mediaway/tree/main/bindings/cpp/examples):

| Capability | Example files |
|------------|---------------|
| Container | `container/mux_roundtrip.cpp` |
| Device | `device/camera_record.cpp` · `capture_microphone.cpp` · `capture_screen.cpp` |
| Pipeline | `pipeline/encode_audio.cpp` · `encode_to_mp4.cpp` · `screen_record.cpp` |

Build and run instructions (CMake): [`bindings/cpp/README.md`](https://github.com/nyxways/mediaway/blob/main/bindings/cpp/README.md).
