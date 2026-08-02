# Language bindings

Root folder for Mediaway's planned per-language bindings
([`docs/spec/c-ffi.md`](../docs/spec/c-ffi.md) Tier B/C). Each language gets a
subfolder; the real binding source (once building starts) lives directly
under `bindings/<lang>/`, with hand-written usage samples kept in
`bindings/<lang>/examples/`.

**Status:** the C ABI (`mediaway-common-ffi`, `mediaway-container-ffi`,
`mediaway-pipeline-ffi`, `mediaway-device-ffi`) is real and
hardware-verified. `bindings/c/examples/*.c` are real, link+run-verified
against it. `bindings/csharp/src/` has all four planned packages
(`Mediaway.Common`, `Mediaway.Container`, `Mediaway.Pipeline`,
`Mediaway.Device` — Camera/Microphone/DeviceHotplug poll mode; Screen
capture deferred) with xUnit suites that run against the real native
libraries (mux/demux round-trip; real H.264 hardware encode; real camera +
microphone capture) — see
[`docs/adr/0017-csharp-binding-package-layout.md`](../docs/adr/0017-csharp-binding-package-layout.md).
All four also dual-target `net8.0;netstandard2.0` for Unity consumption via
NuGetForUnity, plus a separate, **unverified** `bindings/csharp/unity/com.mediaway.unity/`
UPM package for Unity-specific texture/audio glue — see
[`docs/adr/0018-csharp-netstandard20-unity.md`](../docs/adr/0018-csharp-netstandard20-unity.md).
Every other `bindings/<lang>/` folder is still `examples/`-only — design-only,
nothing there compiles, links, or ships.

Files under `bindings/<lang>/examples/` are **aspirational example code**:
for each planned language, they show the *ideal, idiomatic* API a future
Mediaway binding for that language should aim for — written as if the binding
package already existed. They exist to drive API design from the consumer side
(write the wishful client code first, then shape the real binding to match),
not as a working scaffold — but once real binding source lands next to them,
they become the target these bindings are built to satisfy.

Each language's `examples/` covers the same 4 scenarios: 3 of the pipeline
examples in [`../examples/`](../examples/) (there are more now, split by
sector — see that folder's own layout), plus one device-capture scenario
(camera) that mirrors `screen_record.*`'s shape without a matching Rust
*pipeline* example of its own yet (`examples/device/capture_camera.rs` covers
camera capture alone, not encode + mux):

| File (name varies by language convention) | Mirrors | Capability |
|---|---|---|
| `mux_roundtrip.*` | [`examples/container/mux_demux_mp4.rs`](../examples/container/mux_demux_mp4.rs) | sans-io container mux + demux roundtrip |
| `encode_to_mp4.*` | [`examples/pipeline/encode_to_mp4.rs`](../examples/pipeline/encode_to_mp4.rs) | auto video encoder → fragmented MP4 |
| `screen_record.*` | [`examples/pipeline/screen_record.rs`](../examples/pipeline/screen_record.rs) | screen + mic capture → encode → MP4 (video + audio) |
| `camera_record.*` | (same shape as `screen_record.*`) | camera + mic capture → encode → MP4, via `mediaway-device`'s `CaptureSource::Camera` |

`camera_record.*` reuses the exact same platform-agnostic `record(...)`
function shape as `screen_record.*` — only the capture source differs — to
make the point that swapping a capture backend requires no change to the
recording loop itself.

## Languages

| Folder | Language | Interop (per `docs/spec/c-ffi.md`) |
|---|---|---|
| [`c/examples/`](c/examples/) | C | Direct — the C ABI itself |
| [`cpp/examples/`](cpp/examples/) | C++ | Thin RAII wrapper over the C ABI |
| [`csharp/examples/`](csharp/examples/) | C# | P/Invoke over the C ABI (net8.0 `LibraryImport` + netstandard2.0 `DllImport` for Unity) |
| [`python/examples/`](python/examples/) | Python | `ctypes`/`cffi` over the C ABI |
| [`zig/examples/`](zig/examples/) | Zig | `@cImport` of the C ABI |
| [`go/examples/`](go/examples/) | Go | `cgo` over the C ABI |
| [`swift/examples/`](swift/examples/) | Swift | C bridging header over the C ABI |
| [`kotlin/examples/`](kotlin/examples/) | Kotlin (Java-interop friendly) | JNI over the C ABI |
| [`nodejs/examples/`](nodejs/examples/) | Node.js (TypeScript) | Native addon / FFI over the C ABI (**not** WASM) |
| [`browser/examples/`](browser/examples/) | Browser (TypeScript) | WASM (`wasm-bindgen`) + native Web APIs (**not** the C ABI) |

Node.js and the browser are **two distinct hosts** for JS/TS with two distinct
interop paths — see `docs/spec/c-ffi.md` § Tier C. Do not collapse them.

## Rules these examples follow

- English comments only, per [`AGENTS.md`](../AGENTS.md) language policy.
- Map existing Rust surfaces; do not invent capabilities the Rust side doesn't have.
- Opaque handles + error codes at the raw C ABI layer; each language wrapper
  translates that into its own idiomatic error handling (exceptions, `Result`,
  error unions, ...).
- Not part of the Cargo workspace: not built, linted, or tested by CI.

Durable changes to the *real* API surface still require an ADR
([`docs/adr/0004-c-ffi.md`](../docs/adr/0004-c-ffi.md)); this folder is
exploratory input to that process, not a substitute for it.
