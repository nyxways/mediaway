# Examples

Every guide in this book is paired with runnable examples in the repository. The Rust
workspace holds the canonical set under [`examples/`](https://github.com/nyxways/mediaway/tree/main/examples),
and the bindings directories mirror them for each supported host language:

| Language | Interop path | Examples |
|----------|--------------|----------|
| [Rust](./rust.md) | Native crates | [`examples/`](https://github.com/nyxways/mediaway/tree/main/examples) |
| [C](./c.md) | Direct C ABI (`mediaway-ffi`) | [`bindings/c/examples/`](https://github.com/nyxways/mediaway/tree/main/bindings/c/examples) |
| [C++](./cpp.md) | Thin RAII C ABI | [`bindings/cpp/examples/`](https://github.com/nyxways/mediaway/tree/main/bindings/cpp/examples) |
| [C#](./csharp.md) | P/Invoke | [`bindings/csharp/examples/`](https://github.com/nyxways/mediaway/tree/main/bindings/csharp/examples) |
| [Python](./python.md) | `ctypes` / `cffi` | [`bindings/python/examples/`](https://github.com/nyxways/mediaway/tree/main/bindings/python/examples) |
| [Node.js](./nodejs.md) | Native Addon / N-API (koffi FFI today) | [`bindings/nodejs/examples/`](https://github.com/nyxways/mediaway/tree/main/bindings/nodejs/examples) |
| [Browser](./browser.md) | WASM + WebCodecs (no C FFI) | [`bindings/browser/examples/`](https://github.com/nyxways/mediaway/tree/main/bindings/browser/examples) |

Examples are grouped by capability, mirroring the guides:

- `container/` — mux + demux only (no codec, no capture)
- `encode/` / `decode/` — one codec direction, no container
- `device/` — one capture source, no encode
- `pipeline/` — composed end-to-end flows (capture → encode → mux, or decode → trim → re-encode)

The language pages below list each language's example files and how to run them.
