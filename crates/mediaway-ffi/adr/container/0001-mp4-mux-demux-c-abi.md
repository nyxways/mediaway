# ADR-0001: MP4 mux/demux C ABI surface (first pass)

- **Status**: Proposed
- **Date**: 2026-07-31
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-container-ffi`

## Context

`mediaway-container-ffi` is the **first** `mediaway-*-ffi` crate in the workspace
(ADR-0004, [`docs/spec/c-ffi.md`](../../../docs/spec/c-ffi.md)). It wraps the real,
hardware-verified fragmented-MP4 mux/demux surface in
[`crates/mediaway-container/src/mp4.rs`](../../mediaway-container/src/mp4.rs)
(`mp4::Muxer<S>` typestate, `mp4::Demuxer`), backed by `iso-bmff` and typed with
`mediaway-common` (`StreamInfo`, `Packet`, `CodecKind`, `Rational`).

Nothing about the concrete C ABI shape exists yet: no header, no `extern "C"`
functions, no error-code enum, no ownership contract. `bindings/c/examples/mux_roundtrip.c`
sketches an aspirational naming scheme for this exact surface, written before
this ADR as consumer-side input, not a binding decision. This ADR is the first
real-world application of ADR-0004's abstract rules ("opaque handles + integer
error codes", "no panic across FFI", "document ownership per function") — it
must make each of those concrete for MP4 mux/demux specifically.

Two things ADR-0004 leaves unanswered that this crate must decide:

1. **Panic strategy.** As of Rust 1.71+, an unhandled panic unwinding out of a
   plain `extern "C" fn` aborts the whole host process (defined behavior, not
   UB — but still unacceptable: a Rust bug should not crash an embedding C#/Python
   process). A concrete catch/convert strategy is needed.
2. **Typestate at a flat C handle.** `mp4::Muxer<Open>` → `mp4::Muxer<Live>` is a
   consuming transition (`begin(self) -> Muxer<Live>`) enforced by the Rust type
   system. C has one `mediaway_muxer_t*`, not two types — the boundary must
   enforce the same illegal-states-unrepresentable guarantee at runtime instead.

## Decision

> Adopt the naming scheme from `bindings/c/examples/mux_roundtrip.c` for
> function/type names, **with four corrections** (below) driven by verifying it
> against the actual Rust types. Hand-write the header. Every buffer that
> crosses the boundary is either a caller-owned borrow (documented "valid for
> this call only") or a library-owned buffer freed through a matching `_free`
> function — never an implicit borrow that outlives the call.

### 1. Opaque handles — single `Box`, no extra indirection

```rust
// mediaway-container-ffi internal representation (never exposed to C).
struct MuxerHandle {
    poisoned: bool,           // set true iff a panic was caught on this handle
    state: MuxerState,
}

enum MuxerState {
    Open(mp4::mux::Muxer<mp4::mux::Open>),
    Live(mp4::mux::Muxer<mp4::mux::Live>),
}

struct DemuxerHandle {
    poisoned: bool,
    inner: mp4::Demuxer,
}
```

`mediaway_muxer_create` does exactly one allocation:
`Box::into_raw(Box::new(MuxerHandle { .. })) as *mut mediaway_muxer_t`.
`mediaway_muxer_close` does exactly one deallocation:
`drop(Box::from_raw(ptr as *mut MuxerHandle))`. No `Rc`/`Arc`/nested `Box` — the
handle **is** the boxed struct; `mediaway_muxer_t` / `mediaway_demuxer_t` are
forward-declared incomplete C structs (`typedef struct mediaway_muxer
mediaway_muxer_t;`, no member list), so C code can pass the pointer around but
never inspect or reconstruct its layout. This keeps the Rust-side layout free to
evolve pre-1.0 without touching the header (ADR-0004 rule 5).

Enforcing `Open → Live` without a second variant of "poisoned-in-between": the
existing `impl Default for Muxer<Open>` lets `mediaway_muxer_begin` do
`let owned = std::mem::take(open_muxer); state = MuxerState::Live(owned.begin());`
in one step — there is no intermediate invalid state to represent, so no third
enum variant is needed for the *normal* path. The `poisoned` flag exists solely
for the *panic* path (§6), a separate concern from typestate.

### 2. Status enum

```c
typedef enum mediaway_status {
    MEDIAWAY_STATUS_OK              = 0,
    MEDIAWAY_STATUS_INVALID_ARGUMENT = 1, /* null pointer, out-of-range index, mismatched ptr/len */
    MEDIAWAY_STATUS_INVALID_STATE    = 2, /* typestate violation: add_track on Live, push/flush/poll on Open */
    MEDIAWAY_STATUS_INVALID_TRACK    = 3, /* iso_bmff::Error::InvalidTrack */
    MEDIAWAY_STATUS_INVALID_PACKET   = 4, /* iso_bmff::Error::InvalidPacket */
    MEDIAWAY_STATUS_INVALID_DATA     = 5, /* iso_bmff::Error::InvalidData */
    MEDIAWAY_STATUS_UNKNOWN_ERROR     = 6, /* iso_bmff::Error is #[non_exhaustive]; catch-all for a future variant */
    MEDIAWAY_STATUS_INTERNAL_PANIC    = 7, /* this call caught a Rust panic; handle is now poisoned */
    MEDIAWAY_STATUS_HANDLE_POISONED   = 8, /* a previous call already poisoned this handle; call refused */
} mediaway_status_t;
```

`INVALID_ARGUMENT`/`INVALID_STATE` are FFI-layer inventions (the Rust API
represents both as compile-time impossibilities); everything else maps directly
onto `iso_bmff::Error` (re-exported as `mp4::Error`), whose 3 variants were
confirmed by reading `crates/iso-bmff/src/error.rs` before writing this ADR.
`UNKNOWN_ERROR` exists **because** that enum is `#[non_exhaustive]`: adding a
Rust-side variant later must degrade to a generic code here, not a broken match.

### 3. Function list

```c
uint32_t mediaway_container_ffi_abi_version(void);

/* — muxer — */
mediaway_muxer_t *mediaway_muxer_create(void);
mediaway_status_t mediaway_muxer_add_video_track(mediaway_muxer_t *muxer,
                                                  const mediaway_video_track_info_t *info);
mediaway_status_t mediaway_muxer_add_audio_track(mediaway_muxer_t *muxer,
                                                  const mediaway_audio_track_info_t *info);
mediaway_status_t mediaway_muxer_begin(mediaway_muxer_t *muxer);
mediaway_status_t mediaway_muxer_push_packet(mediaway_muxer_t *muxer,
                                              const mediaway_packet_view_t *packet);
mediaway_status_t mediaway_muxer_flush(mediaway_muxer_t *muxer);
mediaway_status_t mediaway_muxer_poll_bytes(mediaway_muxer_t *muxer,
                                             uint8_t **out_data, size_t *out_len);
void mediaway_muxer_close(mediaway_muxer_t *muxer);

/* — demuxer — */
mediaway_demuxer_t *mediaway_demuxer_create(void);
mediaway_status_t mediaway_demuxer_push_bytes(mediaway_demuxer_t *demuxer,
                                               const uint8_t *data, size_t len);
size_t mediaway_demuxer_stream_count(const mediaway_demuxer_t *demuxer);
mediaway_status_t mediaway_demuxer_stream_at(const mediaway_demuxer_t *demuxer,
                                              size_t index,
                                              mediaway_stream_info_t *out_info);
mediaway_status_t mediaway_demuxer_poll_packet(mediaway_demuxer_t *demuxer,
                                                mediaway_packet_t *out_packet,
                                                bool *out_has_packet);
void mediaway_demuxer_close(mediaway_demuxer_t *demuxer);

/* — shared frees — */
void mediaway_buffer_free(uint8_t *data, size_t len);
void mediaway_packet_free(mediaway_packet_t *packet);
void mediaway_stream_info_free(mediaway_stream_info_t *info);
```

`set_decryption_key`/`clear_decryption_key` (`DemuxDecrypt`) exist on
`mp4::Demuxer` but are **out of scope for this pass** — deferred, not dropped
(§ Deferred). `Muxer::with_fragment_batch` is likewise deferred.

`mediaway_demuxer_stream_count` + `stream_at(index)` (index access) is adopted
from the aspirational example instead of a single "get all streams" call — the
latter would need array-of-owned-`mediaway_stream_info_t` ownership (each with
its own owned `extra_data`), a second free convention this pass avoids.

### 4. Corrections to the aspirational example

Verifying the sketch against the real Rust types surfaced four issues. All four
are **adjustments**, not silent divergences — reasons below, per the design
brief's requirement to address why the existing sketch doesn't work as-is.

| # | Aspirational sketch | Problem | Correction |
|---|---|---|---|
| a | `mediaway_buffer_free(uint8_t *data)` — pointer only | Freeing needs the length to reconstruct `Box<[u8]>` (`Box::from_raw` on a slice needs a fat pointer). A length-less free would need a hand-rolled length-prefixed allocation header — pure `unsafe` bookkeeping for no benefit, since every real call site already has the length sitting next to the pointer. | `mediaway_buffer_free(uint8_t *data, size_t len)` |
| b | `mediaway_muxer_add_video_track(muxer, &info, &out_track_id)` implies the **library** assigns the id | `iso_bmff::mux::Muxer::add_track` (verified in `crates/iso-bmff/src/mux/mod.rs`) takes `track.id` as **input**, only checks uniqueness, and echoes it back. The **caller** assigns ids. | `id: u32` moves into `mediaway_video_track_info_t`/`mediaway_audio_track_info_t` as an input field; drop the `out_track_id` out-param (status code alone tells success/failure) |
| c | One `mediaway_packet_t` used for both `push_packet` input and `poll_packet` output, `const uint8_t *payload` | Input is a caller-owned borrow (valid for the call only); output must be library-owned and freed (`mediaway_packet_free`). A `const` pointer can't later be freed, and reusing one struct for both hides which direction owns the buffer. | Split into `mediaway_packet_view_t` (input, `const uint8_t *payload`, no free needed) and `mediaway_packet_t` (output, owned `uint8_t *payload`, freed via `mediaway_packet_free`) |
| d | `mediaway_rational_t.den` cast to `int32_t` (seen in `encode_to_mp4.c`, a different aspirational crate) | `mediaway_common::Rational` is `{ num: u64, den: u32 }`, not `{ i32, i32 }` — verified in `crates/mediaway-common/src/lib.rs`. | `mediaway_rational_t { uint64_t num; uint32_t den; }` |

Everything else — `mediaway_muxer_t`/`mediaway_demuxer_t` opaque pointers,
`mediaway_status_t`/`MEDIAWAY_OK`, the six-verb muxer lifecycle, the
`mediaway_buffer_free`-style ownership hand-off shape, `<mediaway/container.h>`
— is adopted as-is.

### 5. Struct layouts

```c
typedef struct mediaway_rational {
    uint64_t num;
    uint32_t den;
} mediaway_rational_t;

typedef enum mediaway_codec_kind {
    MEDIAWAY_CODEC_H264 = 0,   MEDIAWAY_CODEC_HEVC = 1,
    MEDIAWAY_CODEC_AV1  = 2,   MEDIAWAY_CODEC_VP9  = 3,
    MEDIAWAY_CODEC_AAC  = 4,   MEDIAWAY_CODEC_OPUS = 5,
    MEDIAWAY_CODEC_MP3  = 6,   MEDIAWAY_CODEC_VORBIS = 7,
    MEDIAWAY_CODEC_WEBVTT = 8, MEDIAWAY_CODEC_TX3G = 9,
    MEDIAWAY_CODEC_RAW_VIDEO = 10, MEDIAWAY_CODEC_RAW_AUDIO = 11,
} mediaway_codec_kind_t; /* mirrors mediaway_common::CodecKind 1:1 — pre-1.0, values may be renumbered */

typedef struct mediaway_video_track_info {
    uint32_t id;                 /* caller-assigned; unique per muxer (see §4b) */
    mediaway_codec_kind_t codec;
    mediaway_rational_t time_base;
    uint32_t width;
    uint32_t height;
    const uint8_t *extra_data;   /* borrowed; valid for the call only; NULL iff extra_data_len == 0 */
    size_t extra_data_len;
} mediaway_video_track_info_t;

typedef struct mediaway_audio_track_info {
    uint32_t id;                 /* caller-assigned; unique per muxer */
    mediaway_codec_kind_t codec;
    mediaway_rational_t time_base;
    uint32_t sample_rate;
    uint16_t channels;
    const uint8_t *extra_data;   /* borrowed; valid for the call only */
    size_t extra_data_len;
} mediaway_audio_track_info_t;

/* Input to mediaway_muxer_push_packet — borrowed view, no free function. */
typedef struct mediaway_packet_view {
    uint32_t stream_id;
    int64_t pts;
    int64_t dts;
    uint64_t duration;
    bool is_keyframe;
    bool is_discard;
    const uint8_t *payload;   /* borrowed; valid for the call only */
    size_t payload_len;
} mediaway_packet_view_t;

/* Output of mediaway_demuxer_poll_packet — owned; release with mediaway_packet_free. */
typedef struct mediaway_packet {
    uint32_t stream_id;
    int64_t pts;
    int64_t dts;
    uint64_t duration;
    bool is_keyframe;
    bool is_discard;
    uint8_t *payload;         /* owned */
    size_t payload_len;
} mediaway_packet_t;

/* Output of mediaway_demuxer_stream_at — owned extra_data; release with mediaway_stream_info_free. */
typedef struct mediaway_stream_info {
    uint32_t id;
    mediaway_codec_kind_t codec;
    mediaway_rational_t time_base;
    bool has_geometry;
    uint32_t width;           /* valid only if has_geometry */
    uint32_t height;          /* valid only if has_geometry */
    uint32_t sample_rate;     /* 0 if not applicable — mirrors StreamInfo::sample_rate's Some(0) meaning "N/A", never "silence" */
    uint16_t channels;        /* 0 if not applicable */
    uint8_t *extra_data;      /* owned */
    size_t extra_data_len;
} mediaway_stream_info_t;
```

### 6. Memory ownership

- **`push_packet` / `add_*_track` (input):** payload/`extra_data` are borrowed
  C-owned buffers, valid only for the duration of the call. The FFI layer does
  exactly one copy at the boundary (`bytes::Bytes::copy_from_slice`) to build
  the owned `Packet`/`StreamInfo` the Rust core keeps. **This is a copy path,
  not Zero-Copy** — `Packet.payload` is `Bytes` on the Rust side specifically
  to make sharing cheap *within* Rust, but C has no refcounted-buffer concept
  to hand across the boundary without inventing one; that would need an
  explicit `GpuBufferHandle`-style shared-CPU-buffer ABI type, which does not
  exist yet (§ Deferred). Do not describe this path with a `zc`/⚡ label.
- **`demuxer_push_bytes` (input):** same shape — `iso_bmff::Demuxer::push_bytes`
  already does `self.buffer.extend_from_slice(chunk)` (verified in
  `crates/iso-bmff/src/demux/mod.rs`), i.e. the Rust core copies synchronously
  before returning; the FFI layer adds no *extra* copy here, it only builds a
  transient `&[u8]` view over the caller's pointer for the call's duration.
- **`poll_bytes` (output):** hands back an **owned** buffer
  (`uint8_t **out_data, size_t *out_len`), confirming the aspirational example
  (§4 table, unchanged). Each call drains "whatever fragment bytes are ready
  right now" into a fresh `Vec<u8>`, then `into_boxed_slice()` +
  `Box::into_raw()` to leak ownership to the caller; `mediaway_buffer_free`
  reconstructs and drops the `Box<[u8]>`. `into_boxed_slice()` may itself
  reallocate/copy once if the `Vec`'s spare capacity isn't already zero — a
  known, documented cost. Alternative considered: a caller-provided
  growable-buffer API (avoids that one shrink-copy) — rejected for this first
  pass because it pushes a resize/retry loop onto every language binding for a
  cost that's amortized by calling `poll_bytes` in reasonably sized batches
  (e.g. after `flush()`), not per packet.
- **`poll_packet` (output):** also an owned copy (`mediaway_packet_t.payload`,
  freed via `mediaway_packet_free`), for the same non-ZC reason as above —
  **not** a borrowed view into the demuxer's internal `Bytes`, even though that
  would be genuinely Zero-Copy on the Rust side. A borrowed-view design was
  considered and rejected for v1: it creates a "valid until the next call on
  this handle" lifetime contract that is easy to violate correctly in a
  GC'd host language (Python/C#/Node) and would be the first `unsafe`-adjacent
  footgun this ABI ships. Logged as a deliberate future option (§ Deferred),
  not solved now.
- **`stream_at` (output):** same owned-copy treatment for `extra_data`, freed
  via `mediaway_stream_info_free`. This is not a hot path (queried once per
  track), so the copy is inconsequential.
- **`_free` functions read length from the struct itself** where one exists
  (`mediaway_packet_free`, `mediaway_stream_info_free` read `payload_len` /
  `extra_data_len` off the passed-in struct and null the pointer/len after
  freeing, to make a double-free a visible no-op rather than UB). Only the
  struct-less `mediaway_buffer_free` needs an explicit `len` parameter (§4a).

### 7. Panic safety

Every exported function's body runs inside
`std::panic::catch_unwind(AssertUnwindSafe(|| { .. }))`. `AssertUnwindSafe` is
required because the closure captures `&mut HandleType` behind the raw pointer,
which is not `UnwindSafe` by default — and that default is *correct*: a panic
mid-mutation may leave the Rust-side struct in a logically inconsistent (but
never memory-unsafe) state. Rather than assume otherwise, on a caught panic the
handle's `poisoned` flag is set to `true` and every subsequent call on that
handle short-circuits to `MEDIAWAY_STATUS_HANDLE_POISONED` before touching the
possibly-inconsistent state — **except** `mediaway_*_close`, which must always
be safe to call to release memory. `close` itself is panic-guarded too; in the
unlikely event a panic occurs during `drop`, the `Box` is intentionally leaked
(not double-freed, not aborted) — a documented last-resort, not normal
behavior, and would itself be a Mediaway bug to fix if ever observed.

Null-pointer/argument checks happen **before** entering `catch_unwind` (cheap,
can't panic) and return `MEDIAWAY_STATUS_INVALID_ARGUMENT` directly.

Out of scope for this pass: allocator OOM. Rust's default global allocator
*aborts the process* on allocation failure (`handle_alloc_error`) — this is not
a panic and cannot be caught by `catch_unwind` or represented as a status code.
`mediaway_*_create` returning `NULL` is reserved for a caught panic during
construction (defensive; `Muxer::new()`/`Demuxer::new()` are simple enough that
this should never trigger in practice), not for OOM.

### 8. Header authoring

**Hand-written** `include/mediaway/container.h` in this crate, not
`cbindgen`-generated, for this first pass:

- The design deliberately hides Rust layout behind fully opaque handles and
  hand-picked struct shapes that diverge from a mechanical translation (the
  input/output packet split in §4c has no single corresponding Rust type;
  `cbindgen` has no way to know to make that split).
- `cbindgen` is itself a new dev-dependency requiring deps-policy justification
  ([`docs/conventions/deps-policy.md`](../../../docs/conventions/deps-policy.md))
  — not worth adding for one still-being-designed header on the very first
  `-ffi` crate.
- Revisit in a later ADR once the surface has stabilized and a second/third
  `-ffi` crate exists to justify shared tooling.

Version convention: a compile-time guard/macro pair plus a runtime accessor
(the cdylib is loaded dynamically, so a header-only macro can't be checked by a
Python/Node/Go consumer that never compiles against the header):

```c
#ifndef MEDIAWAY_CONTAINER_H
#define MEDIAWAY_CONTAINER_H
#define MEDIAWAY_CONTAINER_FFI_ABI_VERSION 0  /* bump on any breaking change; pre-1.0, no stability promise */
#ifdef __cplusplus
extern "C" {
#endif
/* ... declarations ... */
#ifdef __cplusplus
}
#endif
#endif /* MEDIAWAY_CONTAINER_H */
```

`mediaway_container_ffi_abi_version()` returns the same integer at runtime, so
a dynamically-loaded consumer can assert the loaded library matches what it was
built against.

### 9. Feature flags

This crate's `Cargo.toml` today has no `[features]` table. Decision: add one
mirroring `mediaway-container`'s own `mux`/`demux` split
(`default = ["mux", "demux"]`), gating the corresponding `extern "C"` functions
with `#[cfg(feature = "mux")]`/`#[cfg(feature = "demux")]` so a slim build
(`--no-default-features --features mux`) genuinely exports fewer symbols, not
just leaves them unused — satisfying `docs/spec/c-ffi.md`'s "CI builds at least
one slim feature set" and this crate's own roadmap Stage 4. This crate's
dependency on `mediaway-container` is pinned to
`default-features = false, features = ["mux", "demux", "audio", "video"]`
(both `audio` and `video` are required — verified in `crates/iso-bmff/Cargo.toml`
that H.264/AAC sample-entry support needs both, they are not optional for this
surface) rather than blindly inheriting `mediaway-container`'s own
`default = ["full"]` — stating explicitly what this crate uses instead of
silently taking everything, per the design brief.

**Explicitly documented limitation, not fixed here:** `mediaway-container`'s
`Cargo.toml` depends on `ebml-webm`, `riff-wave`, `adts`, `mpeg-audio`, `ogg`,
`flv`, `mpeg-ts` **unconditionally** — none of those format cores are
`optional = true` or gated by a Cargo feature, so no feature selection on
`mediaway-container-ffi`'s end can currently produce an MP4-only compiled
artifact; every consumer of this MP4-only C ABI still compiles in WebM/WAV/
ADTS/MP3/Ogg/FLV/MPEG-TS support it never calls. This is a real gap, but it is
`mediaway-container`'s own Cargo.toml structure to fix (a separate, facade-level
concern), not something this FFI-surface ADR can resolve — flagged as a
follow-up against `mediaway-container`'s roadmap.

### 10. Thread safety

Handles are **thread-confined by convention**, not internally synchronized: the
underlying `mp4::Muxer`/`mp4::Demuxer` hold plain `Vec`/`Bytes`/primitives (no
interior mutability), so a handle may be *moved* to another thread, but calling
two functions on the **same** handle pointer concurrently from different
threads without external synchronization is a data race — undefined behavior,
not merely wrong output. This is documented on every function in the header,
not only in this ADR (ADR-0006 "code carries the contract").

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Let panics unwind / abort (no `catch_unwind`) | A Rust bug would crash the entire embedding host process (C#/Python/Node) instead of surfacing as one failed call — unacceptable for a library edge |
| Borrowed/view-based `poll_packet` and `stream_at` output (real Zero-Copy) | Genuinely faster, but introduces a "valid until next call" lifetime contract that's an easy UAF footgun in GC'd host languages; deferred until a concrete perf need + a safer handle-scoped view API design exist |
| `cbindgen`-generated header | Mechanical translation can't express the input/output packet-struct split or opaque-handle hiding this ADR chose; adds a new dep for one still-evolving header |
| Single `mediaway_packet_t` for both directions (as sketched) | `const`-incorrect for output (must be freed) or unfree-able for input (never owned) — ambiguous ownership from the type alone |
| Library-assigned track ids + `out_track_id` (as sketched) | Doesn't match `iso_bmff::mux::Muxer::add_track`'s actual contract — the caller already assigns ids, the library only validates uniqueness |
| Length-less `mediaway_buffer_free(uint8_t*)` (as sketched) | Requires a hand-rolled length-prefixed allocation header purely to save one `size_t` argument every call site already has on hand |

## Consequences

### Positive

- Concrete, reviewable ABI surface for the first `-ffi` crate; every open
  question ADR-0004 left abstract now has a stated answer.
- Panic-safety and typestate-at-a-flat-handle strategies are reusable
  precedent for the next `-ffi` crate (encoder, device, ...).
- Ownership is uniform and simple: everything crossing out of Rust is either a
  call-scoped borrow or an owned buffer with one matching free function —
  easy to bind correctly from Python/C#/Go/Node without extra safety
  machinery, at the cost of a documented (non-hot-path) copy on `poll_packet`
  and `stream_at`.

### Negative / Trade-offs

- `poll_packet`/`stream_at` are copy paths, not Zero-Copy, despite the Rust
  core underneath using refcounted `Bytes` — a real, though currently
  unavoidable, cost surrendered at this boundary (§6, §Deferred).
- `mediaway-container-ffi` cannot yet ship an MP4-only compiled artifact
  because of `mediaway-container`'s unconditional format-core dependencies
  (§9) — a structural gap outside this ADR's scope.
- Four corrections to the already-published aspirational example (§4) mean
  `bindings/c/examples/mux_roundtrip.c` itself will need a follow-up edit once
  implementation starts, to stay accurate to the real ABI.

## Deferred to a later ADR / explicit open questions

- **ClearKey decrypt** (`set_decryption_key`/`clear_decryption_key`) — real
  Rust surface exists (`DemuxDecrypt`), intentionally left out of this pass's
  function list.
- **`Muxer::with_fragment_batch`** — a second `mediaway_muxer_create_with_fragment_batch(size_t)`
  constructor, not added yet.
- **Borrowed/Zero-Copy output variant** for `poll_packet`/`stream_at` (§6) —
  logged as a known future option once a concrete consumer needs the
  performance and a safe handle-scoped view/lifetime API can be designed for
  it, or once a shared `mediaway-common-ffi` Zero-Copy buffer-handle ABI type
  exists to model it explicitly (mirroring `GpuBufferHandle`, but CPU-side).
- **`cbindgen` adoption** — revisit once ≥2 `-ffi` crates exist to justify the
  shared dev-dependency.
- **`mediaway-common-ffi`** (whether/what to share with `mediaway-ffi`)
  — resolved by
  [`docs/adr/0015-common-ffi-unification.md`](../../../docs/adr/0015-common-ffi-unification.md):
  the shared crate unifies the `MediawayRational`/`MediawayCodecKind`
  value-type mirrors and the buffer leak/reclaim helper *implementation* only,
  as an `rlib`-only internal dependency with no C symbols of its own —
  `MediawayStatus` and this crate's own exported `mediaway_buffer_free` name
  are unaffected. The **Zero-Copy buffer-handle** idea in the bullet above is a
  separate, still fully open question that ADR-0015 does not resolve.
- **`mediaway-container`'s unconditional format-core deps** (§9) — a facade-level
  Cargo.toml fix, tracked against that crate's own roadmap, not this ADR.
- **Panic hook behavior** — the default Rust panic hook prints to stderr before
  `catch_unwind` runs; whether this crate should install a custom hook
  (redirect/suppress) is left as Rust's default for now.

## References

- [`crates/mediaway-container-ffi/README.md`](../README.md), [`docs/roadmap.md`](../docs/roadmap.md)
- [`crates/mediaway-container/src/mp4.rs`](../../mediaway-container/src/mp4.rs) — wrapped Rust surface
- [`crates/iso-bmff/src/error.rs`](../../iso-bmff/src/error.rs), [`crates/iso-bmff/src/mux/mod.rs`](../../iso-bmff/src/mux/mod.rs), [`crates/iso-bmff/src/demux/mod.rs`](../../iso-bmff/src/demux/mod.rs) — verified contracts (`add_track` id ownership, `push_bytes` copy semantics)
- [`crates/mediaway-common/src/lib.rs`](../../mediaway-common/src/lib.rs) — `StreamInfo`, `Packet`, `CodecKind`, `Rational` field types
- [`bindings/c/examples/mux_roundtrip.c`](../../../bindings/c/examples/mux_roundtrip.c) — aspirational naming input (non-binding)
- [`docs/spec/c-ffi.md`](../../../docs/spec/c-ffi.md), [`docs/adr/0004-c-ffi.md`](../../../docs/adr/0004-c-ffi.md) — workspace policy this ADR concretizes
- [`docs/spec/zero-cost-abstractions.md`](../../../docs/spec/zero-cost-abstractions.md) — single-`Box` handle shape justification
- [`docs/spec/caveats-and-clarity.md`](../../../docs/spec/caveats-and-clarity.md) — honest-copy-path documentation requirement

ADRs are **English**. Numbering is local to this `adr/` folder.
