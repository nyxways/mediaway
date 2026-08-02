# ADR-0001: AMD AMF (`shiguredo_amf`) vendor encode — deferred, no implementation this stage

- **Status**: Accepted (decision: **defer implementation**, research + placement recorded)
- **Date**: 2026-07-29
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder-amf` (proposed, **not yet a workspace member** — this `adr/`
  directory is the only file this ADR adds; no `Cargo.toml`/`src/` exists)

## Context

Root README's [`GPU — by vendor`](../../../README.md#gpu--by-vendor) table lists AMD H.264/
HEVC/AV1 as 🛠️ (planned, not started) — calling AMD's own **AMF** (Advanced Media Framework)
SDK directly, distinct from the OS·GPU (WMF hardware MFT) and GPU·API (D3D11/Vulkan interop)
axes already covered elsewhere. This ADR researches the one realistic Rust binding and decides
whether to implement a backend now.

**Environment constraint (central to this decision):** the development machine used for this
research has an NVIDIA RTX 4090 and an Intel UHD 770 — **no AMD GPU on any OS**. Unlike the
sibling NVIDIA/Intel vendor-SDK research (parallel, not visible to this session), there is no
AMD silicon or AMD driver available to exercise at all, on Windows or Linux.

## Research: `amf-rs` / `shiguredo_amf`

### Naming hazard — read before touching `Cargo.toml`

The GitHub repo is `github.com/shiguredo/amf-rs`, but **the crates.io package name is
`shiguredo_amf`, not `amf-rs`.** A crates.io package literally named `amf-rs` already exists —
it is a **completely unrelated** project (`F2077/amf-rs`, an *Action Message Format* / Flash
AMF protocol implementation) licensed **GPL-3.0-or-later**. `cargo deny` would catch a GPL dep
at `pre-push`/CI, but `cargo add amf-rs` by name alone silently pulls the wrong, copyleft crate.
**Any future work in this area must depend on `shiguredo_amf` by exact package name**, never
`amf-rs`.

### Review (`docs/conventions/deps-policy.md` checklist)

| Question | Answer |
|----------|--------|
| Correct crate | `shiguredo_amf` (crates.io), repo `github.com/shiguredo/amf-rs` |
| License | **Apache-2.0** (confirmed: `LICENSE` file, standard Apache 2.0 text; `Cargo.toml` `license = "Apache-2.0"`). No GPL/LGPL exposure from this crate itself. |
| Runtime loading | **`dlopen`** of `libamfrt64.so.1` at runtime (bundled with the AMD GPU driver) — **no static link of a proprietary blob**, matching the workspace's "no `libav*` linking" spirit even though AMF isn't FFmpeg-family. Mirrors `cros-libva`'s driver-`.so`-at-runtime model, not a vendored binary. |
| Build-time headers | `build.rs` runs `git clone --depth 1 --branch v1.5.2 https://github.com/GPUOpen-LibrariesAndSDKs/AMF` into `$OUT_DIR`, then `bindgen`s the public headers (`amf/public/include/`). AMD's own AMF repo is **MIT-licensed** — no license issue in the cloned content either. **But** this is a **live network `git clone` at every build** (not a pinned crates.io/registry dependency, not a `deny.toml`-visible git source) — a materially different hermeticity story than `cros-libva` (apt-installed system `libva-dev` headers via `pkg-config`, no network fetch). Needs `git` on PATH and GitHub egress on every clean build/CI run. `DOCS_RS` env var short-circuits to dummy bindings for docs.rs builds only. |
| Platforms | **Linux x86_64 only.** AMD's actual AMF SDK is cross-platform (Windows + Linux), but this specific safe Rust binding targets Linux only today — no Windows support in `shiguredo_amf` as of `2026.3.0`. |
| Published / maintained | Yes — `shiguredo_amf` on crates.io, latest `2026.3.0` (2026-06-23), prior releases roughly monthly since `2026.1.0-canary.0` (2026-03-25), ~6,900 cumulative downloads. Actively maintained by Shiguredo Inc. (also publish `aom-rs`, `shiguredo_webrtc`, etc.). |
| MSRV / edition | `rust-version = "1.93"`, `edition = "2024"`. **This workspace pins `rust-version = "1.85"`** (`Cargo.toml` `[workspace.package]`). **Hard, concrete blocker independent of hardware**: this crate cannot be added under the current workspace MSRV policy without a separate, cross-cutting MSRV bump — out of scope for a single-crate decision. |
| API surface (best effort, not verified against real headers) | `Encoder`/`EncoderConfig` (+ per-codec `H264EncoderConfig`/`HevcEncoderConfig`/`Av1EncoderConfig`), push-frame (`FrameFormat` input, CPU pixel formats: NV12/YV12/I420/BGRA/…) + poll-output (`EncodedFrame`) shape — closer to WMF's push/pull `IMFTransform` model than VA-API's `Picture<S,T>` typestate. No GPU/DX/Vulkan surface type was confirmed from the public docs pass done for this ADR; would need direct source review before an implementation ADR. |
| Alternatives | Hand-written `bindgen` against AMD's own AMF headers directly in a Mediaway crate: strictly more `unsafe` surface owned by this workspace, same live-git-clone-for-headers problem either way (no vendored/cached header option exists upstream), no advantage over depending on `shiguredo_amf`'s already-built safe wrapper. |

## Decision

> **Defer implementation.** Do not create/register a `mediaway-encoder-amf` crate, do not add
> `shiguredo_amf` to any `Cargo.toml`, and do not write encoder code this stage. This ADR
> records the research and the recommended future placement so a later session does not
> re-derive it, but ships **no code** and **no workspace member**.

### Why this is not "implement structurally like VA-API, mark 🛠️ pending hardware"

`mediaway-encoder-linux` ADR-0001 (VA-API via `cros-libva`) set a real precedent for shipping a
structurally-complete, unit/compile-tested backend with **zero real-hardware verification**,
left 🛠️ in README pending hardware. AMD AMF is superficially the same situation (no hardware to
verify against) but differs in ways that change the call:

| Factor | VA-API (`cros-libva`, shipped) | AMD AMF (`shiguredo_amf`, this ADR) |
|--------|-------------------------------|--------------------------------------|
| Hardware blocker | WSL2's VA-API is *currently broken* (`vainfo` segfaults) — an **environment** problem; an Intel UHD 770 is already available and VA-API-capable on real native Linux, so there is a concrete, ownable path to eventual verification without new hardware purchases | No AMD GPU is currently available on any OS — an **ownership** problem, not fixable by fixing WSL or dual-booting |
| Build hermeticity | Builds against apt-installed system `libva-dev` headers (`pkg-config`) — deterministic, no network fetch at build time | `build.rs` does a live `git clone` of AMD's header repo on every clean build — network + `git` required, weaker reproducibility, needs its own sign-off |
| MSRV | `cros-libva` had no MSRV conflict with workspace `1.85` | `shiguredo_amf` requires `1.93` > workspace's pinned `1.85` — **cannot compile under current policy at all**, independent of hardware |
| Platform stacking | First Linux HW encode path for this workspace | Would be a **second** Linux vendor-HW encode path, stacked on top of a first (VA-API) that itself has zero real-hardware confirmation yet — priority order says land VA-API's real verification before adding a second, harder-to-verify Linux path |
| Packaging precedent | `mediaway-<capability>-<platform>` is an established naming-table row (ADR-0012) | "Vendor SDK" crates (`mediaway-encoder-<vendor>`) have **no naming-table row yet**; sibling NVENC/QuickSync research is happening in parallel and invisible to this session — three agents independently inventing `mediaway-encoder-<vendor>` crates in the same batch risks inconsistent packaging that needs reconciling later |

Any one of the MSRV mismatch or the platform-stacking argument would already justify deferral;
together with zero-hardware and the build-hermeticity cost, implementing now would mean shipping
code that cannot even `cargo check` under the workspace's current toolchain policy, for a path
that has no hardware available to verify it at all, ahead of finishing the Linux path that already shipped.

## Planned direction (non-binding — for whoever picks this up later)

Recorded so a future implementation ADR does not restart research from zero. **Not a commitment,
not reviewed against real `shiguredo_amf` source in depth:**

- **Crate**: `mediaway-encoder-amf` — a **vendor-scoped** crate (not `mediaway-encoder-<os>`),
  matching the README "GPU — by vendor" axis being orthogonal to OS. Internally
  `cfg(all(target_os = "linux", target_arch = "x86_64"))`-gate the `shiguredo_amf` dependency
  and real encode path, exactly like `mediaway-encoder-linux` gates `cros-libva` to
  `target_os = "linux"` — non-Linux (and non-x86_64-Linux) builds compile as an honest
  `EncodeError::Unsupported` stub. If AMD ever ships a Windows-capable Rust binding, the crate
  name would not need to change (it is vendor-scoped, not OS-scoped by design).
- **ZCA/typestate sketch** (mirrors `WindowsVideoEncoder`/`LinuxVideoEncoder`'s shape — concrete
  object wrapped in `Option` as a closed-after-move sentinel, no `Box<dyn _>`):
  `AmfVideoEncoder { inner: Option<amf::AmfVideoEncoder> }`, where `amf::AmfVideoEncoder` owns
  the loaded `AmfLibrary` handle + a codec-specific `Encoder` session. Push/poll shape (`push_frame`
  / `poll_packet` / `flush`) matches the existing `VideoEncoder` trait directly — `shiguredo_amf`'s
  own push-input/poll-output model reads closer to WMF's `IMFTransform` than to VA-API's
  `Picture<S,T>` compile-time-ordered typestate, so no new typestate machinery is obviously
  needed, but this needs confirming against real source before committing to it.
  CPU NV12 upload would be a documented `upload_cpu_nv12`-style copy (same cost-disclosure
  convention as the Windows/Linux backends); GPU Zero-Copy input (if `shiguredo_amf` exposes a
  DX11/Vulkan surface import — unconfirmed from the docs pass) would map to
  `GpuBufferHandle::Vulkan` on Linux, deferred either way pending that confirmation.
- **Prerequisites before an implementation ADR can proceed**, in rough order:
  1. Real AMD GPU + driver available somewhere (Windows *or* Linux) for at least an
     honest-skip-vs-actually-runs verification pass — mirrors the VA-API caveat pattern, but
     currently blocked at "no AMD silicon is available" rather than "the
     environment's driver path is broken."
  2. Workspace `rust-version` bumped past `1.93` (its own cross-cutting decision — MSRV is a
     workspace-wide policy change, not a single-crate one) **or** `shiguredo_amf` lowering its
     MSRV in a future release.
  3. `mediaway-encoder-linux`'s own VA-API path getting a real-hardware verification pass first,
     so this workspace does not carry two unverified Linux vendor/HW encode paths at once.
  4. A cross-cutting look at vendor-SDK crate naming (`mediaway-encoder-nvenc`,
     `mediaway-encoder-amf`, `mediaway-encoder-quicksync`, …) — ideally a `docs/adr/` entry once
     more than one vendor-SDK crate is proposed, so naming/packaging is decided once, not three
     times independently. Out of scope for this crate-local ADR to declare unilaterally.
  5. Direct review of `shiguredo_amf`'s real public API (this ADR's "API surface" row above is a
     docs.rs summary pass, not a source read) — confirm push/poll semantics, error types, and
     whether any GPU surface import exists, before writing a struct-field-accurate ADR like
     VA-API ADR-0001's.

## Alternatives Considered

| Alternative | Why not (now) |
|-------------|----------------|
| Implement structurally like VA-API ADR-0001, mark 🛠️ pending hardware | MSRV mismatch (`1.93` vs workspace `1.85`) means it would not even compile under current policy; stacks a second unverified Linux vendor-HW path on an already-unverified first one; see comparison table above |
| Hand-written `bindgen` FFI against AMD's headers directly in this workspace | Same live-git-clone-for-headers problem (AMD does not publish a stable header tarball release cadence this workspace controls), strictly more owned `unsafe` surface than depending on `shiguredo_amf`'s existing safe wrapper, no upside identified |
| Wait for a Windows-capable AMF binding, target Windows first (platform order priority) | No such binding exists today; `shiguredo_amf` is Linux-only. Revisit if AMD or a maintainer ships one. |
| Add `amf-rs` (bare name) as a placeholder dep | Would pull the **wrong, GPL-3.0-or-later crate** — rejected outright, documented here specifically to prevent this mistake |

## Consequences

### Positive

- No code, no workspace member, no dependency added — zero risk to `cargo deny`,
  `cargo check --workspace`, MSRV, or build hermeticity this session.
- Research (correct crate identity, license, dynamic-load model, MSRV conflict, naming hazard)
  is captured once, in the crate-local location a future implementer will look first.
- README's existing 🛠️ (planned, not started) marking for AMD stays accurate — no
  over-promising.

### Negative / Trade-offs

- AMD vendor-HW encode remains unimplemented; no progress on that README cell this session.
- A future implementer must still do the deeper `shiguredo_amf` source review (API surface,
  possible GPU surface import) that this ADR explicitly did not complete.
- The MSRV and vendor-naming prerequisites are cross-cutting and outside this crate's own
  authority to resolve — this ADR can only flag them, not close them.

## References

- [`docs/conventions/deps-policy.md`](../../../docs/conventions/deps-policy.md)
- [`docs/spec/crate-packaging.md`](../../../docs/spec/crate-packaging.md) — no vendor-SDK
  naming-table row exists yet (flagged above)
- [`docs/spec/caveats-and-clarity.md`](../../../docs/spec/caveats-and-clarity.md) — honesty
  requirement this ADR follows (no code implies no capability claim)
- `mediaway-encoder-linux` [ADR-0001](../../mediaway-encoder-linux/adr/0001-vaapi-cros-libva-h264-cpu-upload.md)
  — the VA-API "implement with zero hardware verification" precedent this ADR compares against
- `mediaway-encoder-windows` ADR-0002 (`windows` crate precedent for depending on an official/
  community binding over hand-rolled FFI)
- [`shiguredo_amf` on crates.io](https://crates.io/crates/shiguredo_amf) ·
  [GitHub](https://github.com/shiguredo/amf-rs) (Apache-2.0)
- [AMD `GPUOpen-LibrariesAndSDKs/AMF`](https://github.com/GPUOpen-LibrariesAndSDKs/AMF) (MIT) —
  header source `shiguredo_amf`'s `build.rs` clones at build time
- Root README § [GPU — by vendor](../../../README.md#gpu--by-vendor) · `docs/roadmap.md`
  platform order (Windows → Web → Linux → other)
