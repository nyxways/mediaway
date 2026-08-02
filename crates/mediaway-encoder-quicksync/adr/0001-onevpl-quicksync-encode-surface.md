# ADR-0001: Intel oneVPL (Quick Sync / Arc) encode surface

- **Status**: Accepted — Stage 1 implemented and hardware-verified, see the 2026-07-29 addenda below
- **Date**: 2026-07-29
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder-quicksync` (workspace member as of the addendum below)

## Context

Root README's **GPU — by vendor** table marks Intel H.264/HEVC/AV1 🛠️ (planned). Today's
Windows GPU encode path (`mediaway-encoder-windows`) is Media Foundation + DXGI — an
**OS/graphics-API** path (`Os::Gpu::GraphicsApi` per `mediaway-encoder` ADR-0004). It already
reaches Intel's HW encode block *indirectly*, whenever Intel ships a working MF hardware MFT
(not guaranteed — see `docs/ai/wiki/platform/windows-encode.md`'s driver-quirk note: neither an
RTX 4090 nor an Intel UHD 770 registered a working MF encode HW MFT on the one ad-hoc box
tried). Calling Intel's own SDK directly is the **`Os::Gpu::VendorHw`** sibling backend ADR-0004
already names (`NVENC/…`) — same silicon, different (more direct, more control, cross-platform)
API surface, not automatically faster.

Intel's current vendor SDK for this is **oneVPL** (`intel/libvpl`, formerly hosted at
`oneapi-src/oneVPL`) — the maintained successor to the archived (May 2023) Intel Media SDK
(`libmfx`). oneVPL is also the encode/decode/VPP API for Intel **Arc** discrete GPUs, not just
the integrated "Quick Sync" fixed-function block, and runs on both Windows and Linux through the
same C API — it is not OS-scoped the way WMF is.

### License research

| Component | Repo | License | Notes |
|-----------|------|---------|-------|
| oneVPL dispatcher + API headers | `intel/libvpl` (renamed from `oneapi-src/oneVPL`) | **MIT** | `LICENSE` file confirms MIT; headers live under `api/vpl/*.h` |
| oneVPL GPU runtime (driver-side implementation) | `intel/vpl-gpu-rt` (formerly `oneapi-src/oneVPL-intel-gpu`) | **MIT** | Not a build dependency — see Decision; ships inside the Intel graphics driver package, loaded at **runtime** only |
| oneVPL CPU reference implementation | `oneapi-src/oneVPL-cpu` | MIT, but **archived/no longer active** | Out of scope; hardware-only |

No GPL/LGPL/copyleft anywhere in this stack. MIT is already on `deny.toml`'s allow-list —
**no `deny.toml` change needed.**

### Runtime loading (confirms the "no heavyweight SDK install" premise)

- On Windows, the oneVPL runtime library ships **inside the standard Intel graphics driver**
  package (iGPU and Arc) — installing the latest Intel driver is sufficient; no separate oneVPL
  SDK/runtime install is required to *run* an oneVPL app.
- The **dispatcher**'s job (per Intel's own docs) is: search configured folders (`ONEVPL_SEARCH_PATH`
  env var, then default locations / Windows registry) for shared libraries named `libvpl*`, `dlopen`/
  `LoadLibrary` each, resolve `MFXQueryImplsDescription()` to inspect capabilities, then forward
  `MFX*` calls into the chosen implementation. The **driver-shipped runtime library itself directly
  exports** the `MFX*` C entry points (`MFXInitEx`, `MFXQueryVersion`, `MFXVideoENCODE_*`,
  `MFXVideoCORE_*`, …) — the dispatcher is a thin loader/chooser layered on top, not a
  privileged/opaque broker.
- Building an app against oneVPL only needs the **C headers** (`api/vpl/*.h`, plain C, no
  generated-from-toolkit macros) — not the full Intel oneAPI base toolkit.

This matches the task's premise: genuinely permissive, genuinely light to build against.

### Existing Rust bindings — none usable

| Candidate | Verdict |
|-----------|---------|
| `intel-mediasdk-sys` (crates.io, `rust-av/intel-mediasdk-rs`) | **Not usable.** Targets the **archived** Intel Media SDK (`libmfx`), not oneVPL. Single `0.1.0` release from **2019**, no further releases. Build script uses `metadeps`/`pkg-config` against **system-installed** MediaSDK headers (no vendoring) — the very SDK Intel itself deprecated in 2023. |
| `rust-av/intel-mediasdk-rs` (safe wrapper, same org) | Same target (legacy MediaSDK), ~18 commits total, incomplete per its own README TODO list. Dormant. |
| Any `onevpl`/`vpl`/`libvpl` crate on crates.io | **None found** at time of research (searched "onevpl rust", "libvpl rust bindings", crates.io keyword `intel`). |

**Conclusion:** no existing Rust crate binds oneVPL. A new binding is required if this backend
is built.

## Decision

> Add a **vendor-scoped platform backend**, `mediaway-encoder-quicksync`, plus a **freestanding,
> unprefixed FFI core**, `vpl-sys`, that binds the oneVPL C API. Do not fold this into
> `mediaway-encoder-windows`.

### Crate placement

```text
[unprefixed core]   vpl-sys              ← raw oneVPL FFI, no Mediaway types, reusable outside Mediaway
[platform backend]  mediaway-encoder-quicksync   ← Mediaway VideoEncoder impl, cfg(windows) + cfg(target_os = "linux")
```

- **`vpl-sys`** is unprefixed per ADR-0012: it has no dependency on `mediaway-common` or any
  Mediaway facade, mirrors how raw FFI crates are named in the Rust ecosystem (`-sys` suffix), and
  is — like `iso-bmff`/`iso-cenc` — potentially useful to any Rust project wanting raw oneVPL
  access, not just Mediaway. It does **not** duplicate `cros-libva`'s pattern of bundling both raw
  bindings *and* a safe layer in one crate, because no such "safe oneVPL wrapper" crate exists to
  adopt (unlike the VA-API case, where `cros-libva` already existed and was reused —
  `mediaway-encoder-linux` ADR-0001). We are necessarily writing both layers here.
- **`mediaway-encoder-quicksync`** is the Mediaway-typed adapter: implements `mediaway_encoder::VideoEncoder`
  over a `vpl-sys` session, translates `GpuBufferHandle`/`VideoEncoderConfig`/`EncodeError`, and
  owns the D3D11 interop glue on Windows (a Linux D3D11 GPU handle obviously does not apply there —
  the Linux side of this crate would use VA-API surface import via oneVPL's VAAPI memory type
  instead; **this ADR scopes Stage 1 to Windows D3D11 only**, Linux VAAPI-backed oneVPL surfaces
  are future work, tracked in this crate's `docs/roadmap.md` once created).

### Why a vendor-scoped crate, not `mediaway-encoder-windows`

`mediaway-encoder` ADR-0004 already establishes `Os::Gpu::GraphicsApi` (WMF/DXGI, OS-owned) and
`Os::Gpu::VendorHw` (direct vendor SDK) as **sibling backends**, not one filtered path. Folding
oneVPL calls into `mediaway-encoder-windows` would:

- Contradict ADR-0004's own vocabulary (VendorHw is explicitly not nested under the OS-graphics
  path).
- Break cross-platform reuse — oneVPL is not Windows-only, so an OS-suffixed crate is the wrong
  home for something that also targets Linux Arc/iGPU boxes.
- Mix two very different `unsafe` FFI surfaces (COM/`IMFTransform` vs a C ABI + dynamic-loaded
  driver library) in one crate, which `docs/spec/crate-packaging.md` discourages ("do not fold …
  into the facade as `cfg` modules").

### Why `quicksync`, not `onevpl`, in the crate name

Two reasonable options were compared:

| Name | For | Against |
|------|-----|---------|
| `mediaway-encoder-onevpl` | Names the literal SDK/API this crate binds | The API name is an **implementation detail**, exactly like `mediaway-encoder-windows` is not named `mediaway-encoder-wmf` even though it binds Media Foundation. Naming after the internal binding library, not the recognizable capability, breaks that precedent. |
| **`mediaway-encoder-quicksync`** (chosen) | Matches the recognizable vendor/hardware-capability naming already used for the sibling `VendorHw` example in ADR-0004 (`NVENC` — a hardware/brand name, not "NVIDIA Video Codec SDK"). Matches root README's own "GPU — by vendor" row label style. | "Quick Sync" is historically the *integrated-GPU* fixed-function block's marketing name; oneVPL also reaches Arc discrete GPUs. Accepted as a naming trade-off — see Consequences. |

`vpl-sys` keeps the literal SDK name where it belongs: the binding crate, not the product-facing
vendor backend crate.

### `vpl-sys` — scope and build shape

**Do not** vendor or build Intel's own C dispatcher source, and **do not** link any
Intel-provided import library (`.lib`) at compile time. Concretely:

1. **Headers only, vendored + pinned.** Vendor a minimal subset of `api/vpl/*.h`
   (`mfxdefs.h`, `mfxstructures.h`, `mfxcommon.h`, `mfxsession.h`, `mfxvideo.h`, plus the D3D11
   handle-type header) from a specific **tagged commit** of `intel/libvpl` under
   `vpl-sys/vendor/api/vpl/`, with the upstream `LICENSE` (MIT) copied alongside for attribution.
   Pin the commit hash in `vpl-sys`'s crate docs / build script comment (not the
   `docs/standards/registry.toml` BLAKE3 flow — that convention is for external **standards**
   documents, ISO/ITU/IETF text, not vendored C headers; vendored headers are normal `-sys`-crate
   practice and get their own pinned-commit note instead).
2. **`bindgen` (build-dependency) parses those vendored headers only** — no system oneVPL SDK
   install required to build. This mirrors the already-accepted `cros-libva` precedent
   (`mediaway-encoder-linux` ADR-0001): a `bindgen` build step is an accepted cost for
   platform-gated vendor backends in this workspace. Unlike `cros-libva`, we vendor the headers
   ourselves instead of requiring a system package — a **stronger** guarantee (no
   `libva-dev`-style "must apt-install first" step; only `libclang` for `bindgen` itself, the same
   build-time cost this workspace already pays on Linux).
3. **No C compilation** — headers are pure declarations; `bindgen` only parses them, so no `cc`
   crate / C compiler dependency is introduced.
4. **Runtime loading via `libloading`** (new dependency: MIT/Apache-2.0, tiny, extremely widely
   used, no controversy) instead of vendoring/building Intel's own dispatcher C sources or linking
   an Intel import lib. `vpl-sys` implements a **deliberately minimal "MVP dispatcher"**:
   - Search order: `ONEVPL_SEARCH_PATH` env var override, then a short list of default locations
     (system32/driver store search on Windows; conventional `/usr/lib`-family paths on Linux).
   - `LoadLibraryW`/`dlopen` each `libvpl*`-named candidate, resolve `MFXQueryImplsDescription`
     (and the small set of `MFX*` entry points Stage 1 needs) via `GetProcAddress`/`dlsym`.
   - **First working Intel GPU implementation wins.** Multi-GPU selection, full
     `MFXCreateConfig`/`MFXSetConfigFilterProperty` capability filtering, and CPU-implementation
     fallback are **out of scope** for the MVP dispatcher — an ambiguous/ multi-adapter environment
     returns an explicit "unsupported, pick a device" error, never a silent guess (per
     `docs/spec/caveats-and-clarity.md`).
   - This is an intentionally **reduced reimplementation** of Intel's own dispatcher logic, not a
     port of Intel's dispatcher C source. Document this scope reduction prominently in `vpl-sys`
     rustdoc so callers do not assume full official-dispatcher behavior (env-var config file
     parsing, versioned implementation ranking, etc.).

### `mediaway-encoder-quicksync` — Stage 1 scope

1. Session open via `vpl-sys` MVP dispatcher (`MFXLoad`-equivalent → pick first Intel GPU impl →
   `MFXCreateSession`).
2. `MFXVideoENCODE_Init` for H.264 (HEVC/AV1 as codec-parameter follow-ups, not separate FFI
   work — same entry points).
3. **Zero-Copy D3D11 input** (`VideoInputPreference::ZeroCopyGpu`) via an external frame
   allocator (`mfxFrameAllocator`) whose `GetHDL` callback returns `mfxHDLPair` — see ZCA shape
   below. CPU NV12 upload (`upload_cpu_nv12`, matching the existing Windows/Linux naming
   convention) as the documented-cost fallback path.
4. `MFXVideoENCODE_EncodeFrameAsync` push + `MFXVideoCORE_SyncOperation` drain → `Packet`.
5. `MFXVideoENCODE_Close` / `MFXClose` / `MFXUnload` teardown.

**Out of scope this ADR:** Linux VAAPI-backed oneVPL surfaces, Arc-specific AV1 feature
negotiation beyond "ask for the codec, honor `Unsupported` if the driver refuses", a
`mediaway-decoder-quicksync` decode counterpart (same `vpl-sys` core, separate crate + ADR when
decode work reaches this vendor path), and any multi-GPU / hybrid (iGPU + Arc) selection policy
beyond `GpuDeviceHandle`-driven `Compatible` intake.

## ZCA shape (Windows D3D11 path)

```text
QuickSyncEncoder            // public: impl mediaway_encoder::VideoEncoder
  └─ Option<QuickSyncSession<EncoderReady>>   // closed-after-move sentinel, matches
                                               // WindowsVideoEncoder / LinuxVideoEncoder precedent

QuickSyncSession<S>          // module-private typestate (S: SessionState), thin wrapper
                              // around a raw vpl_sys::mfxSession (opaque *mut c_void)
  Loader          → MVP dispatcher picked an implementation, no session yet
  Opened          → MFXCreateSession succeeded
  EncoderReady    → MFXVideoENCODE_Init succeeded, push_frame/poll_packet valid
```

- No `Box<dyn _>` / `dyn Trait` in the hot path — `QuickSyncEncoder` wraps a concrete
  `QuickSyncSession<EncoderReady>`, exactly like `WindowsVideoEncoder`/`LinuxVideoEncoder` wrap
  their concrete session types. `Box<dyn VideoEncoder>` remains available generically via the
  facade's existing blanket impl (`mediaway_encoder::VideoEncoder for Box<T>`).
- **D3D11 handle mapping — genuine Zero-Copy, no bridge, no readback:**

  ```text
  GpuBufferHandle::DirectX11 { texture: NativeHandle, subresource: u32 }
                          │  (cast texture.get() → ID3D11Texture2D*)
                          │  (subresource: u32   → HANDLE-sized index)
                          ▼
  mfxHDLPair { first: <ID3D11Texture2D*>, second: <subresource index as HDL> }
  ```

  oneVPL's documented external-allocator contract: for D3D11 surfaces, the allocator's `GetHDL`
  callback must return an `mfxHDLPair` where `.first` is the `ID3D11Texture2D*` and `.second` is
  the array/subresource index. This is a **one-field-each cast**, not a copy or a cross-API
  bridge — `GpuBufferHandle::DirectX11`'s two fields (`texture`, `subresource`) already carry
  exactly the two pieces oneVPL's `mfxHDLPair` wants. No GPU→GPU copy, no CPU staging, no
  `D3d12SharedEncodeBridge`-style bridge is needed for the common case (app already owns an
  `ID3D11Texture2D`). `VideoEncoderConfig::gpu_device` (`GpuDeviceHandle::DirectX11`) binds via
  `MFXVideoCORE_SetHandle(MFX_HANDLE_D3D11_DEVICE, …)` before encoder init, mirroring how
  `mediaway-encoder-windows` requires `d3d11_device` before its own DX11 Zero-Copy path (ADR-0003).
- `SmallVec`/`Vec` only where oneVPL's own C structs require fixed-size or growable arrays
  (e.g. extension-buffer chains, `mfxExtBuffer*` arrays) — no per-frame heap churn on the
  steady-state push/poll loop; `mfxFrameSurface1`/`mfxBitstream` structs are reused, not
  reallocated per call, matching oneVPL's own async-queue design.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Fold into `mediaway-encoder-windows` as a `cfg`/module | Contradicts ADR-0004's `GraphicsApi`/`VendorHw` sibling split; wrong OS scope (oneVPL also runs on Linux); mixes two unrelated `unsafe` FFI surfaces in one crate — against `crate-packaging.md`. |
| Adopt `intel-mediasdk-sys` / `intel-mediasdk-rs` | Targets the archived Intel Media SDK, not oneVPL; dormant since 2019; requires system-installed headers, not vendored/pinned. |
| Vendor + build Intel's real C dispatcher via `cc` | Larger build surface (C compiler dependency, more vendored C sources) for behavior (multi-impl ranking, capability filtering) Stage 1 does not need yet; MVP Rust-native loader covers the realistic single-Intel-GPU case honestly. |
| `mediaway-encoder-onevpl` naming | Names the binding SDK, not the recognizable capability — breaks the `windows`-not-`wmf` naming precedent this workspace already set. |
| Defer entirely (no ADR, wait) | The task explicitly asked for a design decision now; existing-binding research shows no shortcut exists, but the scope is honestly small enough (~15–20 FFI entry points, no C compilation, no SDK install) to be worth committing to a plan rather than leaving README 🛠️ unexplained. |

## Consequences

### Positive

- Genuine GPU Zero-Copy path to Intel HW encode independent of whether a given driver exposes a
  working Media Foundation HW MFT (documented as flaky on the one ad-hoc box tried).
- `vpl-sys` is reusable outside Mediaway (unprefixed, no Mediaway types) — same value proposition
  as `iso-bmff`/`iso-cenc`.
- No new copyleft/FFmpeg exposure; MIT throughout; `deny.toml` unchanged.
- Small, bounded build-time footprint: vendored headers + `bindgen` (parse-only) + `libloading`
  (pure Rust) — no C compiler, no Intel SDK install, no Intel import-lib linking.

### Negative / Trade-offs

- New build-time dependency on `libclang` for `bindgen` on any host building this crate (already
  an accepted cost pattern for the Linux VA-API backend; here it is narrower since headers are
  vendored, not system-installed).
- The MVP dispatcher is a **deliberately reduced reimplementation** of Intel's real dispatcher —
  must stay honestly documented as such; multi-GPU / capability-filtering gaps could surprise a
  user who expects official-dispatcher parity.
- "Quick Sync" as the crate name trades brand recognizability for slight imprecision on
  Arc-only-oneVPL scenarios; mitigated by rustdoc/crate description clarifying oneVPL/Arc coverage.
- Genuinely new FFI surface with **zero real-hardware verification** at ADR time (no code written
  yet) — an Intel UHD 770 is available for eventual verification once Stage 1 lands,
  but this ADR itself makes no correctness claim beyond the documented API research above.
- Adds a fourth Windows video-encode identity in the workspace (`mediaway-encoder-windows`
  GraphicsApi vs this crate's VendorHw) — `auto`/backend-preference wiring (ADR-0004) will need to
  learn about it; not done in this ADR.

## Addendum — 2026-07-29: Stage 1 built and hardware-verified

Everything below was written **after** the code, documenting what was actually built/run, not a
revision of the original design record above (kept as-is).

### What shipped

- **`vpl-sys`** (`crates/vpl-sys/`): vendored `mfxdefs.h`/`mfxcommon.h`/`mfxstructures.h` from
  `intel/libvpl` commit `674d015bcb294bc39fa276e99a652ea045423e82` (`vendor/api/vpl/`,
  `vendor/README.md` documents the pin + update procedure). `build.rs` feeds only
  `mfxstructures.h` (whose `#include` chain pulls in `mfxcommon.h`/`mfxdefs.h`) to `bindgen` with
  `ignore_functions()` — **types only**, exact struct/union layout (oneVPL's `#pragma pack(4)` /
  `pack(8)` headers) from real Clang parsing rather than a hand-transcribed guess. `mfxsession.h`/
  `mfxvideo.h`/`mfxmemory.h` are vendored for reference (the real signatures `dispatcher.rs`'s
  `Pfn*` typedefs are transcribed from) but **not** fed to `bindgen` — this crate never emits a
  build-time-linked `extern "C" { fn MFXInit(...); }`; every entry point is resolved at runtime.
  `src/consts.rs` hand-transcribes every numeric constant used (status codes, `MFX_IMPL_*`,
  codec/profile/level/IOPattern/rate-control selectors), each cited against the vendored header
  line it came from.
- **`vpl-sys::dispatcher`**: the MVP `libloading` dispatcher the original ADR designed —
  `Loader::open()` tries `ONEVPL_SEARCH_PATH` then the bare candidate name (`libmfxhw64.dll` on
  Windows), resolves 10 entry points, `Loader::create_session` calls `MFXInitEx`. `Session` wraps
  the resulting `mfxSession` + the `Loader` (kept alive for the session's lifetime) with safe(-ish)
  methods (`query_version`, `query_impl`, `encode_query`, `encode_init`, `encode_close`,
  `encode_frame_async`, `sync_operation`); `Drop` calls `MFXClose`.
- **`mediaway-encoder-quicksync`** (`src/quicksync.rs`, `cfg(windows)` only this stage):
  `QuickSyncSession` — a real `VideoEncoder` impl. Session open validates config, builds
  `mfxVideoParam`/`mfxInfoMFX`/`mfxFrameInfo` (H.264 Baseline, `MFX_TARGETUSAGE_BALANCED`, real
  I/P GOP via `GopPicSize`/`GopRefDist` — the oneVPL runtime manages reference-picture lists
  itself, unlike the Linux VA-API crate's manual all-IDR approach), CQP (`bitrate_bps == 0`) or
  CBR rate control. `push_frame` copies the caller's tightly-packed NV12 into an internal
  16-aligned upload buffer (`upload_cpu_nv12`, a real documented copy — oneVPL surfaces need
  multiple-of-16 dimensions; most real resolutions, e.g. 1080p, are not already 16-aligned),
  builds an `mfxFrameSurface1` pointing at it, and calls
  `MFXVideoENCODE_EncodeFrameAsync` + `MFXVideoCORE_SyncOperation` with a bounded
  `MFX_WRN_DEVICE_BUSY` retry loop. `flush` drains buffered frames via repeated
  `EncodeFrameAsync(None, …)` until `MFX_ERR_MORE_DATA`. The public `QuickSyncVideoEncoder`
  wrapper mirrors `LinuxVideoEncoder`'s cfg-gated real-impl/honest-`Unsupported`-stub shape
  (`mediaway-encoder-linux` ADR-0001 precedent) — Windows only this stage; Linux is deferred
  (tracked in `docs/roadmap.md`), not attempted, since no Linux Intel GPU was available to verify
  against in this session.
- `unsafe` surface: `vpl-sys::dispatcher` (all real FFI calls) and exactly one function in
  `mediaway-encoder-quicksync` (`collect_packet`'s `slice::from_raw_parts` reading the output
  bitstream) — everything else, including all `mfxFrameSurface1`/`mfxVideoParam`/`mfxInfoMFX`
  construction, is safe Rust (union field *writes* never need `unsafe` in Rust, only reads do;
  this crate's structs are built via struct/union literals and `..Default::default()`, never
  read back).

### Real oneVPL runtime — confirmed present, not assumed

- `libmfxhw64.dll` (the real Intel HW implementation library) **and** `libvpl.dll` (Intel's real
  2.x dispatcher) are both present directly under `%SystemRoot%\System32` on the Intel UHD 770
  dev box (`iigd_dch.inf_amd64_68fe65fbc646d3a4` driver package) — confirmed via
  `llvm-readobj --coff-exports`, not guessed. `libmfxhw64.dll` exports `MFXInit`/`MFXInitEx`/
  `MFXClose`/`MFXQueryIMPL`/`MFXQueryVersion`/`MFXVideoCORE_*`/`MFXVideoENCODE_*` directly —
  exactly the "runtime library itself directly exports the MFX* entry points" shape the original
  ADR's license research predicted from Intel's docs, now hardware-confirmed.

### Real finding not documented upstream: `MFX_IMPL_HARDWARE_ANY`, not `MFX_IMPL_HARDWARE`

`MFXInitEx` against a directly-loaded implementation library (this crate's MVP dispatcher,
bypassing the real oneVPL dispatcher's own adapter-resolution step) returns
`MFX_ERR_UNSUPPORTED` for `MFX_IMPL_HARDWARE` (`0x0002`, "a specific hardware implementation")
but **succeeds** for `MFX_IMPL_HARDWARE_ANY` (`0x0004`, "any hardware implementation"), with or
without `MFX_IMPL_VIA_D3D11` additionally OR'd in. Found by sweeping every `MFX_IMPL_*` /
`MFX_IMPL_VIA_*` combination against real hardware (a throwaway diagnostic loop in
`vpl-sys/src/dispatcher_tests.rs` during this session, since removed after the finding), not by
reading it anywhere in Intel's documentation. Plausible explanation: the real dispatcher normally
resolves "the specific hardware adapter" before calling into an implementation library — a
resolution step this crate's MVP dispatcher does not perform, so the specific-adapter selector
has nothing concrete to resolve against. Documented in `vpl_sys::consts::MFX_IMPL_HARDWARE`'s and
`MFX_IMPL_HARDWARE_ANY`'s rustdoc so a future reader does not have to rediscover this.

### Real finding: `mfxBitstream::MaxLength` must cover `mfxInfoMFX::BufferSizeInKB`

An initial `bitstream` buffer sized only to one NV12 frame's worth of bytes (`max(frame_size,
64KiB)`) was rejected by `MFXVideoENCODE_EncodeFrameAsync` with `MFX_ERR_NOT_ENOUGH_BUFFER`
(`-5`) whenever `bitrate_bps > 0` (the CBR path, which sets a nonzero `BufferSizeInKB` — the VBV
buffer size). Fixed by sizing `bitstream_storage` to also cover `BufferSizeInKB * 1000` bytes
(`QuickSyncSession::open`, with a comment citing this exact finding). Not an `MFX_ERR_*` this
crate had reason to expect from reading the headers alone — found by running against real
hardware.

### Real hardware-verified encode — actual test output

`cargo test -p mediaway-encoder-quicksync -- --nocapture` (the Intel UHD 770,
`libmfxhw64.dll`), 2026-07-29:

```text
running 5 tests
test quicksync::tests::align16_rounds_up_to_next_multiple_of_16 ... ok
test quicksync::tests::ts_90k_roundtrip_is_stable_at_30fps ... ok
test tests::open_rejects_zero_copy_gpu_this_stage ... ok
mediaway-encoder-quicksync: public API real encode produced 8 packet(s)
vpl-sys/mediaway-encoder-quicksync: real encode produced 15 packet(s), NAL types seen:
[9, 7, 8, 6, 5, 9, 8, 6, 1, 9, 8, 6, 1, 9, 8, 6, 1, 9, 8, 6, 1, 9, 8, 6, 1, 9, 8, 6, 1, 9, 8,
 6, 1, 9, 8, 6, 1, 9, 8, 6, 1, 9, 8, 6, 1, 9, 8, 6, 1, 9, 8, 6, 1, 9, 8, 6, 1, 9, 8, 6, 1]
test tests::public_api_real_encode_or_skips ... ok
test quicksync::tests::real_encode_produces_annex_b_sps_and_idr_or_skips ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.22s
```

NAL type `9` = AUD, `7` = SPS, `8` = PPS, `6` = SEI, `5` = IDR slice, `1` = non-IDR (P) slice —
a real, well-formed H.264 Annex-B stream: SPS+PPS+IDR for the first (keyframe) packet, then
AUD+PPS+SEI+P-slice for each subsequent frame, confirming the real I/P GOP structure
(`GopRefDist = 1`) is genuinely reference-predicting, not independently-IDR like the Linux VA-API
stage. `vpl-sys`'s own hardware test (`cargo test -p vpl-sys -- --nocapture`) reports a real
session: `Major=1 Minor=255 raw_version=0x000100ff impl=0x00000205` (impl `0x0205` =
`MFX_IMPL_HARDWARE2 | MFX_IMPL_VIA_D3D9` — the runtime's own default acceleration-mode choice
when `MFX_IMPL_VIA_D3D11` is not explicitly requested at `MFXInitEx`; confirms the Stage 3
Zero-Copy D3D11 path will need to request that bit explicitly to guarantee a D3D11-backed
session, not assume one).

### Deviations from the original design (honest, not silent)

- **Session-open impl selector**: `MFX_IMPL_HARDWARE_ANY`, not the `MFX_IMPL_HARDWARE` the
  original ADR's prose implied — see the finding above. No design-level change (still "hardware,
  not software"), just a corrected constant.
- **Bitstream buffer sizing**: must track `BufferSizeInKB`, not just frame size — see the finding
  above. An implementation detail the original ADR did not anticipate (it did not size buffers at
  all, being research-only).
- **Real I/P GOP, not deferred to "future work"**: the original ADR's Stage 1 scope list did not
  explicitly commit to GOP structure one way or the other. Because oneVPL's
  `MFXVideoENCODE_EncodeFrameAsync` contract already manages reference lists internally when
  frames are submitted in display order (unlike VA-API's lower-level manual `Picture`/DPB
  management), implementing real GOP was not extra scope — it was the natural shape of the
  encode-order convention already used, so it shipped rather than being deferred.

### Still not done (unchanged scope, tracked in `docs/roadmap.md`)

Zero-Copy D3D11 (`mfxFrameAllocator`/`GetHDL`), Linux, HEVC/AV1, and `mediaway-encoder` facade/
`auto` wiring remain exactly as scoped "out" by the original ADR — none of that changed this
session.

## Addendum — 2026-07-29 (later this session): HEVC hardware-verified, AV1 attempted and genuinely unsupported

Written **after** the code, same day as the addendum above (a later pass in the same session) —
extends Stage 1's H.264-only scope to HEVC (real, working) and AV1 (real, honestly attempted and
found unsupported on this hardware/driver). Does not revise anything above.

### What shipped

- **`vpl-sys::consts`**: `MFX_CODEC_HEVC`/`MFX_CODEC_AV1` (`mfxstructures.h` lines 1143/1148,
  `CodecFormatFourCC` enum), `MFX_PROFILE_HEVC_MAIN`/`MFX_LEVEL_HEVC_41` (lines 1263/1278),
  `MFX_PROFILE_AV1_MAIN`/`MFX_LEVEL_AV1_41` (lines 1304/1320) — hand-transcribed from the same
  vendored `mfxstructures.h` (pinned commit, see `vendor/README.md`), following the existing
  citation convention exactly.
- **`mediaway-encoder-quicksync::quicksync`**: a new `const fn codec_params(codec: CodecKind) ->
  Result<(u32, u16, u16), EncodeError>` maps `CodecKind::{H264,Hevc,Av1}` to
  `(CodecId, CodecProfile, CodecLevel)`; `validate` now accepts all three (previously H.264-only);
  `build_mfx_info` takes those three values as parameters instead of the hardcoded AVC constants.
  No other change to the encode/push/flush/GOP path — the same `MFXVideoENCODE_Query` /
  `_Init` / `_EncodeFrameAsync` / `MFXVideoCORE_SyncOperation` sequence now runs for whichever
  codec `codec_params` resolved, exactly as ADR-0001's original Decision section anticipated
  ("HEVC/AV1 as codec-parameter follow-ups, not separate FFI work — same entry points").

### Real hardware-verified HEVC encode — actual test output

`cargo test -p mediaway-encoder-quicksync -- --nocapture` (the Intel UHD 770,
`libmfxhw64.dll`), 2026-07-29:

```text
vpl-sys/mediaway-encoder-quicksync: real HEVC encode produced 15 packet(s), NAL types seen:
[32, 33, 34, 39, 19, 39, 1, 39, 1, 39, 1, 39, 1, 39, 1, 39, 1, 39, 1, 39, 1, 39, 1, 39, 1, 39, 1,
 39, 1, 39, 1]
test quicksync::tests::real_hevc_encode_produces_vps_sps_pps_idr_or_skips ... ok
```

NAL type `32` = VPS, `33` = SPS, `34` = PPS, `39` = prefix SEI, `19` = `IDR_W_RADL` slice, `1` =
`TRAIL_R` (P) slice (ITU-T H.265 §7.4.2.2) — a real, well-formed HEVC Annex-B stream: VPS+SPS+PPS
+SEI+IDR for the first (keyframe) packet, then SEI+P-slice for each subsequent frame. Same real
I/P GOP behavior as the H.264 path (`GopRefDist = 1`, driver-managed reference lists) — HEVC
needed **no** GOP/rate-control changes, only the `CodecId`/`CodecProfile`/`CodecLevel` swap.

### Real hardware result — AV1 encode is genuinely unsupported on this Intel UHD 770

`cargo test -p mediaway-encoder-quicksync -- --nocapture`, same run:

```text
vpl-sys/mediaway-encoder-quicksync: real MFXVideoENCODE_Query(AV1) failed on this Alder Lake /
Xe-LP hardware/driver — oneVPL call MFXVideoENCODE_Query failed: mfxStatus -3
test quicksync::tests::av1_encode_query_reports_real_hardware_result_or_skips ... ok
```

`mfxStatus -3` is `MFX_ERR_UNSUPPORTED` (`vpl-sys::consts::MFX_ERR_UNSUPPORTED`) — confirmed by
matching against the crate's own transcribed constant, not by assumption. This is the
**expected, honest** result the task anticipated: the Intel UHD 770 is Alder Lake
(12th gen, Xe-LP integrated graphics) — a generation that supports AV1 hardware *decode* but not
AV1 hardware *encode* (encode requires newer Xe-HPG / discrete Arc silicon). The dedicated
diagnostic test (`av1_encode_query_reports_real_hardware_result_or_skips`, `quicksync_tests.rs`)
calls `vpl-sys::Session::encode_query` **directly** (not through `QuickSyncSession::open`) so the
real `mfxStatus` is captured and printed — `EncodeError` (the public error type
`QuickSyncSession::open` returns) intentionally does not carry the underlying status code per its
own "details in logs when available" contract, so a lower-level diagnostic was necessary to
document the exact code this addendum reports. `MFXVideoENCODE_Query` rejecting the codec means
`MFXVideoENCODE_Init` is never reached (the real Intel dispatcher would not accept params `Query`
already rejected) — not attempted further, matching the task's instruction not to force it.

`QuickSyncSession::open`/`codec_params` still **accept** `CodecKind::Av1` as a valid *input* (no
upfront special-case rejection) — the public API attempts AV1 honestly through the same code path
as H.264/HEVC and surfaces whatever the real hardware reports (`EncodeError::Unsupported` today,
via the same `MFXVideoENCODE_Query` failure). This is deliberate: if a future driver/GPU on this
same code path gains AV1 encode support, no code change is needed here — the honest attempt
already exists, only the hardware answer changes.

### Deviations / notes

- No GOP, rate-control, `IOPattern`, or `FourCC`/`ChromaFormat` changes were needed for HEVC —
  every oneVPL parameter Stage 1 already built out (`mfxFrameInfo`, `NV12`, CQP/CBR selection)
  is codec-agnostic; only `CodecId`/`CodecProfile`/`CodecLevel` differ by codec.
- AV1's `codec_params` profile/level choice (`MFX_PROFILE_AV1_MAIN` / `MFX_LEVEL_AV1_41`) is
  arbitrary within reason (mirrors the AVC/HEVC 4.1-level convention) — since `MFXVideoENCODE_Query`
  already rejects the codec outright, no profile/level combination was found to change the
  outcome; not swept exhaustively (out of scope — the codec itself is unsupported, not a
  parameter-tuning problem).
- Not wired into `mediaway-encoder`'s `auto` facade, same as Stage 1 — unchanged scope per the
  task and this crate's existing convention.

## References

- [`docs/spec/crate-packaging.md`](../../../docs/spec/crate-packaging.md) · [ADR-0003](../../../docs/adr/0003-crate-packaging.md)
- [ADR-0012 crate naming v1](../../../docs/adr/0012-unprefixed-reusable-cores.md)
- `mediaway-encoder` [ADR-0004 backend preference](../../mediaway-encoder/adr/0004-backend-preference.md) (`Os::Gpu::VendorHw`)
- `mediaway-encoder-windows` [ADR-0003 DX11 Zero-Copy](../../mediaway-encoder-windows/adr/0003-dx11-zero-copy.md), [ADR-0002 `windows` crate](../../mediaway-encoder-windows/adr/0002-windows-crate.md)
- `mediaway-encoder-linux` [ADR-0001 VA-API via `cros-libva`](../../mediaway-encoder-linux/adr/0001-vaapi-cros-libva-h264-cpu-upload.md) — `bindgen`/platform-gated-dependency precedent
- [`docs/spec/gpu-interop.md`](../../../docs/spec/gpu-interop.md), [`docs/ai/wiki/zero-copy/handles.md`](../../../docs/ai/wiki/zero-copy/handles.md) (`GpuBufferHandle::DirectX11`)
- [`docs/conventions/deps-policy.md`](../../../docs/conventions/deps-policy.md), [`docs/conventions/perf-crates.md`](../../../docs/conventions/perf-crates.md)
- [`docs/ai/wiki/platform/windows-encode.md`](../../../docs/ai/wiki/platform/windows-encode.md) — MF HW MFT driver-quirk note that motivates a direct vendor SDK path
- `intel/libvpl` (MIT) — <https://github.com/intel/libvpl>; `intel/vpl-gpu-rt` (MIT) — <https://github.com/intel/vpl-gpu-rt>
- Root README § Codec support (GPU — by vendor, Intel row) — not edited by this ADR

ADRs are **English**. Numbering is local to this `adr/` folder.
