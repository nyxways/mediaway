# ADR-0004: Upgrade `wgpu` from 26.x to 30.x

- **Status**: Accepted — hardware-verified 2026-08-18 (same session)
- **Date**: 2026-08-18
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway` (`wgpu` module — DX12 HAL escape-hatch bridges)

## Context

The workspace `wgpu` pin has been `26.0` since [ADR-0001](0001-dx12-hal-gpucopy-bridge.md)
first landed the DX12 HAL bridge, specifically because `wgpu` 30.x's rustc floor (`1.93`)
exceeded the workspace's then-current `rust-version` pin (`1.91`). [Workspace ADR-0023](../../../../docs/adr/0023-msrv-bump-1-96.md)
raised that floor to `1.96`, clearing the blocker; the user then requested the 26.x→30.x major
bump directly, plus a general dependency refresh pass.

`wgpu` 30.x is a real major-version jump (four minors: 27, 28, 29, 30) with breaking API changes
in exactly the surface this crate's HAL escape-hatch bridges use.

## Decision

> Bump `wgpu = "26.0"` to `wgpu = "30.0"` in the root `Cargo.toml`, fix the resulting breakage in
> `crates/mediaway/src/wgpu/{dx12,dx12_decode}.rs` and their integration tests, and re-verify on
> real hardware (this workspace's reference machine: NVIDIA RTX 4090 + Intel UHD 770).

### Breaking changes found and fixed (empirically, via `cargo check`, not guessed)

1. **`windows`-crate straddle resolved.** `wgpu-hal` 26.x pinned its own `windows`/
   `windows-core` dependency to `0.58`, incompatible as a Rust type with this workspace's
   ordinary `windows = "0.62"` (ADR-0001's "windows-rs version straddle" — worked around with a
   second, explicitly `=0.58.0`-pinned dependency aliased `windows-hal-interop`, used only at
   the two points that talk to `wgpu_hal::dx12` directly). `wgpu-hal` 30.0.0's own `Cargo.toml`
   now pins `windows = "0.62"` — the same line this workspace already uses. The
   `windows-hal-interop` alias and its two `use` sites (`dx12.rs`, `dx12_decode.rs`, and the
   `dx12_decode_pixel_roundtrip.rs` integration test) are removed entirely; those files now
   import `ID3D12Device`/`ID3D12Resource`/`Interface` from the ordinary `windows` dependency,
   same as every other windows-typed file in this crate.
2. **`Device::create_texture_from_hal` gained a required `initial_state: wgt::TextureUses`
   parameter.** Per its own doc comment: pass `TextureUses::UNINITIALIZED` when the wrapped
   resource's existing contents may be discarded/are not yet meaningfully initialized. Both call
   sites (`dx12.rs::wrap_bridge_resource`, `dx12_decode.rs::wrap_bridge_resource`) wrap a
   destination texture whose real content is written by a copy call *after* wrapping
   (`copy_frame`'s `copy_texture_to_texture`, `import_decoded_texture`'s
   `copy_from_decoded`) — `UNINITIALIZED` is the correct, honest value in both cases, not a
   guess papering over an unknown state.
3. **`PollType::WaitForSubmissionIndex(submission_index)` no longer exists.** `wgt::PollType`
   is now `{ Wait { submission_index: Option<T>, timeout: Option<Duration> }, Poll }`. Fixed to
   `PollType::Wait { submission_index: Some(submission_index), timeout: None }` — the same wait
   semantics (block for that specific submission, not just "the most recent"), just a struct
   variant instead of the old unit-style one (ironic, since ADR-0001's own original — wrong —
   guess for 26.x *was* this exact struct shape, "corrected" to the tuple form once real 26.x
   source was checked; 30.x brought the struct shape back for real).
4. **`Instance::new` takes an owned `InstanceDescriptor`, not `&InstanceDescriptor`.** All three
   call sites (`dx12_encode_smoke.rs`, `dx12_decode_smoke.rs`, `dx12_decode_pixel_roundtrip.rs`)
   updated to pass by value.
5. **`InstanceDescriptor` no longer derives `Default`** (its `display: Option<Box<dyn
   WgpuHasDisplayHandle>>` field structurally can't). The replacement is
   `InstanceDescriptor::new_without_display_handle()`, used via `..` struct-update syntax in
   place of the old `..Default::default()`.
6. **`Instance::enumerate_adapters` became `async`**, now returning `impl Future<Output =
   Vec<Adapter>>` instead of `Vec<Adapter>` directly. Both call sites (`dx12_decode_smoke.rs`,
   `dx12_decode_pixel_roundtrip.rs`) wrap it in `pollster::block_on(...)`, the same
   sync-test/async-API pairing this crate's tests already use for `request_adapter`/
   `request_device`.

### Real hardware re-verification (RTX 4090 + Intel UHD 770, this session)

`cargo check --workspace --all-features`, `cargo clippy --workspace --all-targets
--all-features -- -D warnings`, and the full workspace test suite (`cargo test --workspace`,
`--all-features`, including doc-tests) are all clean after the fixes above. Critically, the
three DX12 HAL bridge integration tests were re-run **against real hardware**, not just
compiled:

- `wgpu_dx12_decode_bridge_pixel_roundtrip_or_skip` — **actually ran** (did not skip): real
  byte-exact NV12 pixel round trip, 6144 bytes (64×64), through the DX12→D3D11 decode-import
  bridge.
- `wgpu_dx12_decode_bridge_constructs_on_same_adapter_or_skip` — **actually ran**: real
  same-adapter D3D11/DX12 device pairing succeeded.
- `wgpu_dx12_bridge_encodes_h264_or_skip` — skips with the exact same, already-documented
  reason as under `wgpu` 26.x (`no HW H.264 MFT for BGRA DXGI input`, ADR-0001's own
  "Verification update") — confirmed to be the same pre-existing hardware/driver limitation,
  not a regression introduced by this upgrade.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Stay on 26.x | User explicitly requested the 30.x bump now that ADR-0023 clears the MSRV floor; no remaining technical blocker once the `windows`-crate straddle resolved itself as a side effect of the bump. |
| Bump to an intermediate minor (27/28/29) instead of latest 30.x | No reason found to stop short — 30.x is what the user asked for, compiles clean, and hardware-verifies clean; an intermediate stop would just defer the same API-shape work without benefit. |
| Guess the API fixes from 30.x's public docs without a real local build | Rejected per this workspace's own established discipline (ADR-0001 itself was originally written this way and got 3 things wrong) — every fix above was driven by real `cargo check` compiler feedback against the actual `wgpu-hal-30.0.0`/`wgpu-types-30.0.0` source, then hardware-verified. |

## Consequences

### Positive

- Removes the `windows-hal-interop` straddle dependency entirely — one fewer duplicate
  `windows`-crate version in the dependency graph, simpler `Cargo.toml`, no more
  cross-version-pointer-bit reasoning needed in `dx12.rs`/`dx12_decode.rs`.
- Real hardware re-verification, not just a compile check — the two decode-bridge tests
  actually exercised real GPU work on this upgrade, at the same confidence level ADR-0001's own
  original verification pass established for 26.x.
- Workspace now tracks a current `wgpu` major version rather than one four majors behind.

### Negative / Trade-offs

- `create_texture_from_hal`'s new `initial_state` parameter is a real behavioral surface this
  crate must get right on every future call site that wraps a HAL texture — `UNINITIALIZED` is
  correct for this crate's current two call sites (both destination-only, written after
  wrapping) but a future call site wrapping an *already-populated* resource would need the
  matching real `TextureUses` state, not a copy-pasted `UNINITIALIZED`.
- `wgpu` 30.x is still pre-1.0 — same semver-risk class as before; another future major bump
  will likely repeat this exercise.

## References

- [ADR-0001](0001-dx12-hal-gpucopy-bridge.md) — the original DX12 HAL bridge, its "windows-rs
  version straddle" workaround (now removed), and its own three-bugs verification precedent
- [`docs/adr/0023-msrv-bump-1-96.md`](../../../../docs/adr/0023-msrv-bump-1-96.md) — the MSRV
  bump that cleared `wgpu` 30.x's rustc floor
- `wgpu-hal-30.0.0`, `wgpu-types-30.0.0`, `wgpu-30.0.0` source (read directly from the local
  cargo registry cache to confirm every signature above, not guessed from docs)
- [`docs/conventions/deps-policy.md`](../../../../docs/conventions/deps-policy.md)
