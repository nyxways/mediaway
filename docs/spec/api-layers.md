# API layers — low-level first-class

Mediaway’s high→low stack only works if **low-level APIs are clean, public, and usable on their own**. High-level helpers compose them; they must not bury or lock away the bottoms.

## Layers (top → bottom)

| Layer | What callers get | Must stay public |
|-------|------------------|------------------|
| Convenience / pipeline | “encode this path”, CLI-shaped helpers | Thin; built only from layers below |
| **C ABI (`*-ffi`)** | Opaque handles for C and other languages | Per-capability crates + optional feature umbrella ([`c-ffi.md`](c-ffi.md)); not a Rust substitute |
| Traits / sessions | `Encoder` / `Decoder` / device traits, packet·frame streams | Stable contracts; no hidden side channels |
| Sans-IO cores | Mux/demux push·pull, bitstream transforms, timebase math | Usable without opening files ([`sans-io.md`](sans-io.md)) |
| Shared types | `Rational`, formats, `Packet`/`Frame`, errors | In `mediaway-common` (or re-exported cleanly) |
| Platform handles | `GpuBufferHandle` variants (DX11, WebGPU, …), native session types where needed | Explicit enums/types — not `Any` / erased blobs |

## Rules

1. **Design bottom-up.** Define packet/frame/handle and sans-io or trait surfaces first; add convenience last.
2. **No opaque-only path.** A feature that only works through a high-level black box is incomplete until the low-level surface exists.
3. **Zero-Copy stays reachable.** Apps must be able to push/pull `GpuBufferHandle` (and related fences) **or** shared CPU buffers (e.g. PCM/`Bytes`) without forced payload copy or CPU readback “for the simple API.” Framework users (wgpu, WebGPU, …) use optional adapters ([`gpu-interop.md`](gpu-interop.md)). README **⚡** covers both ([wiki marks](../ai/wiki/zero-copy/marks.md)).
4. **Platform detail is allowed at the bottom.** DX11 texture / WebGPU texture / VA surface may appear as typed variants; unify via traits/enums, don’t erase until the caller asks.
5. **Crate boundaries match layers.** Sans-IO core, facade traits, and `mediaway-*-<platform>` backends are separate crates ([`crate-packaging.md`](crate-packaging.md)). C ABI lives only in `mediaway-*-ffi` / optional `mediaway-ffi` ([`c-ffi.md`](c-ffi.md)). Depend on `common` + the specific crate you need — not a mega-crate.
6. **Docs and examples cover low-level use.** At least one example per capability that never uses the convenience wrapper (Rust). FFI examples come after the C surface exists.
7. **ADRs must name the public low-level surface** (traits, key types, what stays crate-private).

## Anti-patterns

- High-level API owns the only encoder session; low-level is `pub(crate)` forever.
- Mux only accepts `impl Write` to a file; no byte-buffer / pull API.
- GPU frames only as “export to CPU `Vec<u8>` then re-upload.”
- “Unified” API that drops platform handles and only returns CPU frames.

## Relation to vision

Supports **High → low abstraction** and **No performance surrender** in [`vision.md`](vision.md). Complements [`sans-io.md`](sans-io.md): portable cores stay pure; platform bottoms stay explicit and callable. Streaming and async policy: [`async-and-streaming.md`](async-and-streaming.md).
