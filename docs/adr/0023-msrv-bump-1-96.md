# ADR-0023: Bump workspace MSRV from 1.91 to 1.96

- **Status**: Accepted
- **Date**: 2026-08-18
- **Deciders**: @dev-nyxie (+ agent)

## Context

The workspace `rust-version` is currently pinned at `1.91` (bumped from an original `1.85`
bootstrap pin — see `local/agent` MSRV-drift note from 2026-08-07; ADR-0001's `1.85` figure is
now historical). The `mediaway-encoder-amf` research pass
(`crates/mediaway-encoder/adr/amf/0001-amf-deferred-no-hardware.md`) identified a hard,
hardware-independent blocker for AMD AMF vendor encode work: the `shiguredo_amf` crate requires
`rust-version = "1.93"`, above the workspace's `1.91` pin. That ADR explicitly deferred AMD AMF
implementation partly on this basis, listing "workspace `rust-version` bumped past `1.93`" as
one of several prerequisites for a future implementation ADR, and noting it is "its own
cross-cutting decision — MSRV is a workspace-wide policy change, not a single-crate one."

Separately, `Cargo.toml`'s own `wgpu` dependency comment (workspace deps, near the `wgpu = "26.0"`
entry) already flagged that `wgpu` 30.x requires rustc `>= 1.93` and was pinned to the older
26.x minor specifically because it didn't fit the (then) `1.91` floor — a second, independent
data point that the workspace was already brushing up against this exact ceiling.

The user has requested resuming AMD AMF work (alongside greenfield Android/Apple decode) and
has directed the MSRV target to stable `1.96`, i.e. past `1.93`, rather than a narrower bump
to exactly `1.93`. The installed local toolchain (`rustc 1.97.1`) already exceeds `1.96`, so
`1.96` is not a bleeding-edge ask — it is a recent stable already available in CI's usual
`dtolnay/rust-toolchain@stable` resolution and locally.

## Decision

> Bump `[workspace.package].rust-version` from `1.91` to `1.96` across the workspace.

- Update `Cargo.toml` `[workspace.package] rust-version = "1.96"`.
- Update the stale `wgpu` dependency comment that references the old `1.91` figure, so it
  stays accurate (does not itself upgrade `wgpu` past 26.x — that stays a separate, deliberate
  dependency decision per `docs/conventions/deps-policy.md`, out of scope here).
- Update any CI-pinned MSRV check (if a workflow pins an explicit MSRV toolchain distinct from
  `stable`) to `1.96` — none found; workflows resolve `stable`, which already exceeds `1.96`.
- Run `cargo check --workspace --all-features` and `cargo clippy --workspace --all-targets` to
  confirm nothing regresses under the new pin (informational — the pin is a floor, not a
  toolchain change; the local/CI toolchain was already `>= 1.96`).
- Scope: workspace-wide `rust-version` field only. This ADR does not itself add
  `shiguredo_amf` or create `mediaway-encoder-amf` — that remains a separate crate-local
  implementation ADR per `mediaway-encoder-amf`'s (future) `adr/` directory, now unblocked on
  the MSRV axis.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Bump to exactly `1.93` (the `shiguredo_amf` floor) | Less headroom; `1.96` is already the locally installed/CI-available stable and costs nothing further |
| Leave MSRV at `1.85`, vendor/patch `shiguredo_amf` to lower its own MSRV | Not ours to maintain upstream; fragile, and still leaves the workspace on a stale floor for no benefit |
| Defer indefinitely (status quo) | Blocks the AMD AMF work the user has now asked to resume; the crate itself already documented this as the correct unblock path |

## Consequences

### Positive

- Removes the MSRV blocker recorded in `mediaway-encoder/adr/amf/0001-amf-deferred-no-hardware.md`,
  clearing one of that ADR's listed prerequisites for a future AMD AMF implementation ADR.
- Also clears the `wgpu` 30.x ceiling noted in `Cargo.toml`, though upgrading `wgpu` itself is
  deliberately left to a future, separate dependency decision.
- Workspace floor moves closer to the actually-installed toolchain (`1.97.1`), reducing the gap
  between "what CI/local machines run" and "what the crate metadata claims to require."

### Negative / Trade-offs

- Any downstream consumer building against an older toolchain between `1.91` and `1.96` loses
  compatibility. Given this is a pre-1.0, early-development workspace (`docs/spec/status.md`),
  this is an accepted cost, not a breaking-change concern in the stability sense.
- Does not itself resolve the other AMD AMF prerequisites (real hardware access, VA-API
  real-hardware verification precedence, vendor-SDK crate naming) — those remain open and are
  tracked in that crate-local ADR, not here.

## References

- `crates/mediaway-encoder/adr/amf/0001-amf-deferred-no-hardware.md` — the AMF ADR whose MSRV
  blocker this decision clears
- `docs/adr/0001-workspace-bootstrap.md` — original `1.85` pin
- Root `Cargo.toml` `[workspace.package] rust-version`
