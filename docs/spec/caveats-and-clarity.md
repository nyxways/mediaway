# Caveats honesty & code clarity

Canonical decision: [`docs/adr/0006-caveats-and-clarity.md`](../adr/0006-caveats-and-clarity.md).

Two linked goals:

1. **Honest caveats** — performance or compatibility trade-offs are never silent.
2. **Code as primary documentation** — a careful reader of the source should not lack the critical contract.

## Performance / compatibility caveats

If an API, feature flag, or helper can do any of the following, document it **in rustdoc at the item** and, when cross-cutting, in crate docs / ADR / this catalog:

- Copy between GPU APIs (e.g. **OpenGL → D3D11** texture)
- CPU readback or upload staging
- Extra GPU converts when a Zero-Copy path exists elsewhere
- Payload `memcpy` into a new `Vec` on audio/PCM paths sold as Zero-Copy (CPU ⚡ requires shared/borrowed buffers — [wiki marks](../ai/wiki/zero-copy/marks.md))
- Pipeline stalls / blocking maps
- Software codec fallback vs HW
- Quality or color-pipeline loss

### Naming

| Prefer | Avoid when the path copies or stalls |
|--------|--------------------------------------|
| `copy_gl_texture_to_dx11` | `from_gl` (looks free) |
| `readback_texture_to_cpu` | `download` without cost note |
| `compat_…` / `fallback_…` modules | Hiding escapes under `util::convert` |

Defaults must not silently choose a slow path over a fast one.

Also applies to **CPU-side** discipline: avoid casual `.clone()` / per-frame allocation / silent byte copies on hot paths ([`code-style.md`](../conventions/code-style.md) § Allocation, clone, and copy discipline).

### Catalog (fill as implementations land)

| Caveat | Cost | Where documented |
|--------|------|------------------|
| `upload_cpu_nv12` (WMF H.264) | CPU→MF buffer `memcpy` per frame | `mediaway-encoder-windows` rustdoc |
| WASAPI PCM queue | Copies float samples into `Bytes` once per period (not CPU ⚡ — evaluated and rejected, see ADR-0002 addendum) | `mediaway-device-windows` ADR-0002 |
| `webgpu_canvas_frame` (WebCodecs Web) | A raw `GPUTexture` cannot be passed to `VideoFrame` (not a `CanvasImageSource`; confirmed `TypeError` on Chromium). Mediaway renders into a WebGPU-backed `OffscreenCanvas`, then builds the `VideoFrame` from that canvas — GPU-resident on the Mediaway side, but the browser's internal canvas→`VideoFrame` sharing is implementation-defined, not a verifiable Zero-Copy guarantee | `mediaway-encoder-web` rustdoc (`wasm.rs`) |
| `OpusEncoder::push_frame` / `poll_packet`, `OpusDecoder::push_packet` / `poll_frame` (`mediaway-sw-opus`) | Payload `memcpy` across `unsafe-libopus`'s raw `*const f32`/`*mut u8` C-shaped boundary both directions, plus upstream's own ~20% CPU cost vs. the hand-tuned C reference (`c2rust` transpile, no inline asm/SIMD) | `mediaway-sw-opus` rustdoc (`encoder.rs`/`decoder.rs`) |

When you add a slow/compat path, **add a row here** (or a linked crate doc section) in the same change.

## Code clarity (design goal)

Mediaway APIs should be usable from **code + rustdoc alone** for day-to-day work:

- Every public item explains purpose, ownership, errors, and perf/compat notes.
- Domain types beat booleans (`GpuCopyKind`, handle enums).
- `// SAFETY:` is complete enough to review without the ADR open (ADR still required for boundaries).
- Module `//!` describes the layer (sans-io vs platform vs compat escape).

Markdown specs/ADRs remain mandatory for decisions — they must not be the *only* place a footgun is mentioned.

## Review bar

Missing caveat docs or misleading names on a costly path → **Blocking** on review.  
Public API with empty/useless rustdoc on a non-trivial item → **Blocking** (or Non-blocking only for trivial getters).
