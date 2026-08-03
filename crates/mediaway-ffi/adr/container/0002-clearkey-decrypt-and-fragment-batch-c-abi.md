# ADR-0002: ClearKey decrypt + custom fragment batch C ABI surface

- **Status**: Proposed
- **Date**: 2026-07-31
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-container-ffi`

## Context

ADR-0001 shipped the first C ABI pass over `mediaway-container`'s fragmented-MP4
mux/demux surface and explicitly deferred two already-real Rust capabilities
instead of guessing at their C shape:

1. **ClearKey decrypt.** `mp4::Demuxer` (and the underlying `iso_bmff::Demuxer`
   it wraps) already implements
   [`DemuxDecrypt`](../../mediaway-container/src/lib.rs) — verified:

   ```rust
   // crates/mediaway-container/src/lib.rs
   pub trait DemuxDecrypt: Demux {
       fn set_decryption_key(&mut self, key: [u8; 16]);
       fn clear_decryption_key(&mut self);
   }
   ```

   backed 1:1 by [`iso_bmff::Demuxer`](../../iso-bmff/src/demux/mod.rs):

   ```rust
   // crates/iso-bmff/src/demux/mod.rs
   pub const fn set_decryption_key(&mut self, key: [u8; 16]) { self.decryption_key = Some(key); }
   pub const fn clear_decryption_key(&mut self) { self.decryption_key = None; }
   ```

   Neither method takes a track id or a key id (KID) — the key is stored as a
   single `Option<[u8; 16]>` on the `Demuxer` struct itself, not per-track.
   Reading `drain_mdat`/`emit_stbl` in the same file confirms **why**: the
   decrypt call site does

   ```rust
   if let (Some(key), Some(tenc)) = (self.decryption_key, enc.as_ref()) {
       if tenc.is_protected { decrypt_ok = decrypt_sample(&mut payload, key, tenc, senc_s).is_ok(); }
   }
   ```

   — it checks *whether a key is set* and *whether this track is marked
   protected* (`tenc.is_protected`, from the track's `tenc` box), but it never
   compares `key` against `tenc.kid` (the 16-byte KID that **is** parsed and
   stored in [`TrackEncryption::kid`](../../iso-bmff/src/isobmff/cenc_box.rs)).
   So the real Rust surface is: **one demuxer-wide key, applied to every
   protected track, with no KID check.** This is a real, pre-existing property
   of the core — not something this ADR introduces or can silently "fix" by
   inventing a KID parameter that has no Rust-side counterpart
   ([`docs/spec/c-ffi.md`](../../../docs/spec/c-ffi.md) design rule 1: "map
   existing Rust surfaces; do not invent C-only capabilities").

   A second, equally load-bearing fact from the same read: decrypt runs
   **synchronously inside `push_bytes`** (in `drain_mdat`/`emit_stbl`, called
   from `pump()`/`emit_stbl()`), not lazily inside `poll_packet`. If no key is
   set when an encrypted sample is drained, the raw ciphertext is still pushed
   into the packet queue as an ordinary sample with `is_discard: false` —
   `decrypt_ok` starts `true` and is only flipped by an *attempted* decrypt
   that fails inside `decrypt_cenc`. A wrong-but-present key behaves the same
   way: AES-CTR does not detect a wrong key, so `decrypt_sample` still returns
   `Ok(())` and the sample is emitted as if it were valid plaintext. There is
   no error signal for "wrong key" or "no key" at the sample level.

2. **`Muxer::with_fragment_batch`.** [`mp4::Muxer<Open>`](../../mediaway-container/src/mp4.rs)
   exposes a second constructor:

   ```rust
   // crates/mediaway-container/src/mp4.rs
   pub fn with_fragment_batch(batch: usize) -> Self {
       Self { open: Some(IsoMuxer::with_fragment_batch(batch)), .. }
   }
   ```

   which forwards to [`iso_bmff::mux::Muxer::with_fragment_batch`](../../iso-bmff/src/mux/mod.rs):

   ```rust
   pub fn with_fragment_batch(batch: usize) -> Self {
       Self { .., batch: batch.max(1), .. }
   }
   ```

   `batch` is samples-per-fragment; `0` is silently clamped to `1` — there is
   no `Result`, no error path, and no accessor to read the configured value
   back. The returned type is still `Muxer<Open>`, the exact same type
   `Muxer::new()` returns; `begin()`/`push_packet()`/`flush()`/`poll_bytes()`
   are defined on `Muxer<Live>` and `Muxer<Open>` respectively regardless of
   which constructor produced the `Open` value. Nothing about the typestate
   enum in `mediaway-container-ffi::muxer::MuxerState` needs to change.

Neither feature needs a new opaque handle type, and — as the analysis below
shows — neither needs a new `mediaway_status_t` variant either. This ADR is
purely additive to ADR-0001's surface.

## Decision

### 1. ClearKey decrypt attaches to the existing `mediaway_demuxer_t` handle

Two new functions, gated by the crate's existing `demux` Cargo feature (no new
feature flag — see §4):

```c
mediaway_status_t mediaway_demuxer_set_decryption_key(mediaway_demuxer_t *demuxer,
                                                       const uint8_t *key, size_t key_len);
mediaway_status_t mediaway_demuxer_clear_decryption_key(mediaway_demuxer_t *demuxer);
```

No second handle type is introduced: `set_decryption_key`/`clear_decryption_key`
are plain `&mut self` methods on the same `mp4::Demuxer` that `push_bytes`/
`poll_packet` already mutate through `DemuxerHandle::inner` — there is nothing
in the Rust shape that calls for a distinct object.

**Key representation:** borrowed `const uint8_t *key, size_t key_len` (not a
`uint8_t key[16]` fixed array by value), matching the existing borrowed-buffer
convention used for `extra_data`/`payload`/`push_bytes`'s `data`. The FFI layer
validates the length itself:

```rust
let Some(key_slice) = (unsafe { borrow_slice(key, key_len) }) else {
    return MediawayStatus::InvalidArgument; // null with key_len != 0
};
let Ok(key_arr) = <[u8; 16]>::try_from(key_slice) else {
    return MediawayStatus::InvalidArgument; // key_len != 16
};
```

then copies the 16 validated bytes into the owned `[u8; 16]` `mp4::Demuxer::
set_decryption_key` expects — the same "borrow for the call, copy once at the
boundary" shape ADR-0001 §6 already established for `extra_data`/`payload`. A
pointer+length pair (rather than a fixed-size array parameter) means a caller
who mis-sizes their key buffer gets `MEDIAWAY_STATUS_INVALID_ARGUMENT` instead
of the FFI layer reading past the end of a too-short buffer.

**No track/KID parameter.** Grounded in the Context: the real
`DemuxDecrypt`/`iso_bmff::Demuxer` surface has no track- or KID-scoped key
concept to expose. Adding one here would be a C-only invention with no Rust
implementation behind it. See § Deferred.

**No new `mediaway_status_t` variant.** A wrong-length key is the same class
of contract violation ADR-0001 already assigned to `INVALID_ARGUMENT`
("null pointer, out-of-range index, **or mismatched pointer/length pair**") —
here the "pair" is (`key`, `key_len`) against an implicit expected length of
16, which Rust's `[u8; 16]` parameter type makes a compile-time impossibility
and C cannot. This is reused, not duplicated with a new
`MEDIAWAY_STATUS_INVALID_KEY_LENGTH`-style variant (§ Alternatives). Existing
`HANDLE_POISONED`/`INTERNAL_PANIC` apply unchanged; both functions run the
mutation inside `catch_unwind(AssertUnwindSafe(...))` exactly like every other
mutating call in `demuxer.rs`, poisoning the handle on a caught panic, even
though `set_decryption_key`/`clear_decryption_key` are `const fn` field
assignments in Rust and are not expected to ever panic in practice — kept for
uniformity with the rest of the file, not because a panic is likely here.

**Timing contract (must be documented, not just implemented):** decrypt runs
synchronously inside `push_bytes`, so `mediaway_demuxer_set_decryption_key`
only affects samples drained from **subsequent** `push_bytes` calls. Setting
(or clearing) the key after bytes containing the relevant encrypted fragment
were already pushed does **not** retroactively re-decrypt (or re-encrypt)
packets already sitting in the poll queue. This ordering rule has no compiler
enforcement in C and must appear in the header comment for both functions
(ADR-0006 "code carries the contract").

**Honest caveat (must be documented):** because there is no KID check, and
because a wrong-or-missing key does not produce a decode error, multi-KID
content decrypted with the wrong key for even one track — or with no key set
at all — silently yields garbage or raw-ciphertext payload bytes marked as
ordinary, non-discarded samples. This is a pre-existing property of
`iso_bmff::Demuxer`, not introduced by this ABI, but the ABI must not hide it:
the header docs for both functions state this plainly, per
[`docs/spec/caveats-and-clarity.md`](../../../docs/spec/caveats-and-clarity.md)
(ADR-0006).

### 2. Fragment batch becomes a second muxer constructor, no typestate impact

```c
mediaway_muxer_t *mediaway_muxer_create_with_fragment_batch(size_t batch);
```

mirrors `mediaway_muxer_create` exactly — same return-null-only-on-caught-panic
contract, same single allocation, same `MuxerState::Open(...)` variant:

```rust
#[unsafe(no_mangle)]
pub extern "C" fn mediaway_muxer_create_with_fragment_batch(batch: usize) -> *mut MuxerHandle {
    let built = catch_unwind(AssertUnwindSafe(|| MuxerHandle {
        poisoned: false,
        state: MuxerState::Open(mp4::mux::Muxer::with_fragment_batch(batch)),
    }));
    built.map_or(std::ptr::null_mut(), |handle| Box::into_raw(Box::new(handle)))
}
```

`batch: usize` maps to C `size_t` with no conversion (already the convention
for every other length parameter in this crate: `extra_data_len`,
`payload_len`, `key_len` above). `batch == 0` is **not** rejected as
`INVALID_ARGUMENT` — it is passed straight through to
`Muxer::with_fragment_batch`, which itself clamps to `1` (`batch.max(1)`).
Mirroring the core's own definition of "valid" (§ Alternatives) instead of
inventing a stricter FFI-side rejection keeps this function additive: there is
no `Result`/status-code return channel on this constructor to report
`INVALID_ARGUMENT` through anyway (same shape as `mediaway_muxer_create`,
which also returns a bare pointer).

**No other function changes.** `mediaway_muxer_add_video_track`,
`add_audio_track`, `begin`, `push_packet`, `flush`, `poll_bytes`, `close` are
unmodified: they all operate on `MuxerState`, which gained no new variant —
a `Muxer<Open>` built via `with_fragment_batch` is stored in the exact same
`MuxerState::Open(mp4::mux::Muxer<mp4::mux::Open>)` arm as one built via
`new()`. `begin()`'s `std::mem::take(open).begin()` transition (ADR-0001 §1)
is untouched.

**No accessor added.** Neither `mp4::mux::Muxer` nor `iso_bmff::mux::Muxer`
exposes a getter for the configured `batch` value (verified: no such method in
`crates/iso-bmff/src/mux/mod.rs`), so none is added at the C boundary either —
mapping the existing surface, not extending it (`docs/spec/c-ffi.md` design
rule 1).

### 3. Header additions (`include/mediaway/container.h`)

Three new declarations, no new structs, no new enum values:

```c
/* Requires the `demux` feature (already required for every other demuxer function). */
mediaway_status_t mediaway_demuxer_set_decryption_key(mediaway_demuxer_t *demuxer,
                                                       const uint8_t *key, size_t key_len);
mediaway_status_t mediaway_demuxer_clear_decryption_key(mediaway_demuxer_t *demuxer);

/* Requires the `mux` feature. */
mediaway_muxer_t *mediaway_muxer_create_with_fragment_batch(size_t batch);
```

placed next to `mediaway_demuxer_create`/`mediaway_muxer_create` respectively,
each with a doc comment covering: KID/multi-key limitation + timing contract
(decrypt key functions), and the `batch == 0` clamp (fragment-batch
constructor) — condensed versions of the caveats stated above, per ADR-0006
"code carries the contract" (the header, not only this ADR, must state the
footgun).

### 4. No new Cargo feature

Verified in `crates/iso-bmff/Cargo.toml` (`demux = ["dep:iso-cenc"]`) and
`crates/mediaway-container/Cargo.toml` (`demux = ["iso-bmff/demux"]`): ClearKey
decrypt support is already unconditionally bundled into the `demux` feature at
both the `iso-bmff` and `mediaway-container` layers — there is no separate
`cenc`/`clearkey` feature to thread through. `mediaway-container-ffi`'s own
`demux` feature (already enabled by default, already pinned in this crate's
`Cargo.toml`) is sufficient; the two new functions are gated
`#[cfg(feature = "demux")]`, identically to every other function in
`demuxer.rs`. `mediaway_muxer_create_with_fragment_batch` is gated
`#[cfg(feature = "mux")]`, identically to every function in `muxer.rs`.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Separate `mediaway_decryptor_t` handle holding the key, attached to a demuxer at `push_bytes` time | Doesn't match the real shape — `set_decryption_key`/`clear_decryption_key` are plain mutating methods directly on `Demuxer`; a second handle would add indirection with no Rust-side justification |
| Per-track / per-KID key API (`mediaway_demuxer_set_decryption_key_for_track(demuxer, track_id, kid[16], key[16])`) | No such Rust capability exists yet — `iso_bmff::Demuxer` stores exactly one `Option<[u8; 16]>` demuxer-wide and never reads `TrackEncryption::kid` at the decrypt call site. Adding this at the FFI layer alone would be a C-only invention this ADR's grounding rule forbids; the fix belongs in the Rust core first (§ Deferred) |
| Fixed `uint8_t key[16]` array-by-value parameter | A raw pointer + explicit length lets the FFI layer itself catch a mis-sized buffer as `INVALID_ARGUMENT`; a fixed-size parameter shifts that responsibility onto every caller getting the array size exactly right with no length to check against |
| New `MEDIAWAY_STATUS_INVALID_KEY_LENGTH` enum variant | ADR-0001's `INVALID_ARGUMENT` is already scoped to cover "mismatched pointer/length pair" — a wrong-length key is that same class of FFI-invented, compile-time-impossible-in-Rust violation; a dedicated variant would grow the enum for no new information a caller could act on differently |
| Reject `batch == 0` as an FFI-side error | `mediaway_muxer_create_with_fragment_batch` has no status-code return channel (matches `mediaway_muxer_create`'s existing bare-pointer shape); the Rust core already defines `0` as "clamp to 1," not an error — mirroring that instead of inventing stricter FFI-side validation keeps this purely additive |
| Add a `mediaway_muxer_fragment_batch(const mediaway_muxer_t*)` getter | No corresponding Rust accessor exists on either `mp4::mux::Muxer` or `iso_bmff::mux::Muxer` to map |

## Consequences

### Positive

- ClearKey decrypt and custom fragment batching become reachable from C with
  **zero** new handle types, struct types, or status-code variants — pure
  additive surface fully reusing ADR-0001's opaque-handle, panic-safety, and
  status-code conventions.
- `mediaway_muxer_create_with_fragment_batch` has no interaction whatsoever
  with the already-shipped `Open → Live` typestate machinery; nothing in
  `mediaway_muxer_begin`/`push_packet`/`flush`/`poll_bytes`/`close` needs
  re-review.
- The ptr+len key representation keeps every borrowed-buffer parameter in this
  crate's ABI shaped the same way (`data`/`len`, `payload`/`payload_len`,
  `extra_data`/`extra_data_len`, now `key`/`key_len`) — one mental model for
  bindings authors instead of a special case for keys.

### Negative / Trade-offs

- The ClearKey ABI inherits a real, pre-existing Rust-core limitation as-is:
  one global key per demuxer, no per-track KID verification. Multi-KID
  content decrypted with a key that is wrong for even one of several
  differently-keyed tracks — or demuxed with no key set at all — silently
  produces garbage or raw-ciphertext sample payloads marked as ordinary
  (non-discarded) packets, not an error. This is now reachable (and must be
  clearly documented) at the C boundary, not solved by it.
- Callers must set the key **before** the `push_bytes` call(s) that supply the
  affected encrypted fragment(s); this ordering constraint is unenforceable by
  the C type system and depends entirely on header documentation being read.
- `mediaway_muxer_create_with_fragment_batch(0)` silently behaves like
  `mediaway_muxer_create()` (clamped to `1`) rather than surfacing any
  diagnostic — consistent with the Rust core, but a caller passing `0` by
  mistake gets no signal that anything was clamped.

## Deferred to a later ADR / explicit open questions

- **Per-track / per-KID key map.** Needs a new `iso_bmff::Demuxer` capability
  (a `HashMap`/`SmallVec` of `(kid, key)` pairs checked against each track's
  `TrackEncryption::kid` at decrypt time) before an honest multi-KID FFI
  surface can be designed. Tracked against `iso-bmff`'s roadmap; out of scope
  for this FFI-only ADR, which can only map surfaces that already exist.
- **Decrypt failure signaling.** Whether `iso_bmff::Demuxer` should start
  detecting "key almost certainly wrong" (e.g. via an authenticated mode, or a
  container-level integrity box) and surface it as `is_discard: true` more
  reliably is a core-crate design question, not an FFI one.
- **A `mediaway_demuxer_has_decryption_key(const mediaway_demuxer_t*)` query.**
  Not added — no corresponding Rust accessor exists (`decryption_key` is a
  private field on `iso_bmff::Demuxer` with no getter). Could be proposed as a
  small Rust-core addition first if a concrete consumer needs to check state
  without tracking it on their own side.
- **`mediaway_muxer_fragment_batch` getter.** Same reasoning — no Rust
  accessor to map yet.
- Everything ADR-0001 already deferred (borrowed/Zero-Copy `poll_packet`/
  `stream_at` output, `cbindgen` adoption, `mediaway-container`'s
  unconditional format-core deps, panic hook behavior) remains deferred;
  untouched by this ADR.

## References

- [`crates/mediaway-container-ffi/adr/0001-mp4-mux-demux-c-abi.md`](0001-mp4-mux-demux-c-abi.md) — conventions this ADR extends (opaque handles, `MediawayStatus`, panic-safety, ownership)
- [`crates/mediaway-container/src/lib.rs`](../../mediaway-container/src/lib.rs) — `DemuxDecrypt` trait definition
- [`crates/mediaway-container/src/mp4.rs`](../../mediaway-container/src/mp4.rs) — `mp4::Demuxer::set_decryption_key`/`clear_decryption_key`, `mp4::Muxer<Open>::with_fragment_batch`
- [`crates/iso-bmff/src/demux/mod.rs`](../../iso-bmff/src/demux/mod.rs) — `iso_bmff::Demuxer` decryption-key field + `drain_mdat`/`emit_stbl` decrypt call sites (grounds the "no KID check", "synchronous with push_bytes", "wrong/missing key yields non-discarded ciphertext" facts)
- [`crates/iso-bmff/src/isobmff/cenc_box.rs`](../../iso-bmff/src/isobmff/cenc_box.rs) — `TrackEncryption::kid` (parsed, never compared against the demuxer's key)
- [`crates/iso-bmff/src/mux/mod.rs`](../../iso-bmff/src/mux/mod.rs) — `Muxer::with_fragment_batch`, `batch.max(1)` clamp
- [`crates/iso-bmff/Cargo.toml`](../../iso-bmff/Cargo.toml), [`crates/mediaway-container/Cargo.toml`](../../mediaway-container/Cargo.toml) — confirms `demux` already bundles CENC/`iso-cenc` unconditionally; no new feature needed
- [`docs/spec/c-ffi.md`](../../../docs/spec/c-ffi.md), [`docs/adr/0004-c-ffi.md`](../../../docs/adr/0004-c-ffi.md) — design rule 1 ("map existing Rust surfaces; do not invent C-only capabilities")
- [`docs/spec/caveats-and-clarity.md`](../../../docs/spec/caveats-and-clarity.md) — honest-caveat documentation requirement (ADR-0006)

ADRs are **English**. Numbering is local to this `adr/` folder.
