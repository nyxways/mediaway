# ADR-0024: Multi-platform native binding distribution (Linux/macOS)

- **Status**: Accepted — implemented for v0.1.7
- **Date**: 2026-08-20
- **Deciders**: @dev-nyxie (+ agent)

## Implementation note (2026-08-20)

Stage 1 (Linux) and Stage 2 (macOS) landed **together** in v0.1.7, not as
separately merged PRs — an explicit human decision to bundle both into one
release rather than follow the staged rollout below literally. Two
deviations from the design as drafted:

- The npm packaging shape changed from "thin loader + per-platform
  `optionalDependencies` packages" to **bundling every platform's native lib
  directly inside `@mediaway/ffi`** (measured real size: 3.1 MB release
  Windows DLL, 1.9 MB release Linux `.so`, well under the size this ADR
  worried about when it proposed the split — a human call after seeing the
  real numbers, not a correction of a design error).
- The macOS code-signing/Gatekeeper open question (§ Deferred) is **still
  open** — `native-assets-macos`/`bindings-tests-macos` exist in
  `release.yml` but have not run on a real GitHub Actions macOS runner as of
  this note (the branch has not been pushed yet). Treat macOS support as
  authored-but-CI-unverified until that first real run.
- Linux support **was** verified beyond compile-checking this session — real
  WSL2 Ubuntu build + the actual C#/Python/Node/C round-trip tests all ran
  against a real `x86_64-unknown-linux-gnu` release `.so` and passed,
  including installing the built PyPI wheel into a clean venv. This is
  stronger than this ADR's own "compile/lint only" framing assumed Linux
  packaging would ship with.
- **§ Linux glibc baseline was wrong and has been corrected**: the pinned
  `ubuntu-22.04` runner this ADR originally chose turned out to be genuinely
  too old for this workspace's own Linux dependencies, confirmed by two
  distinct real build failures on the actual first `native-assets-linux` CI
  run (not caught by this session's own WSL2 testing, which happened to run
  on Ubuntu 24.04) — `libspa` (a `pipewire` dependency) calls
  `spa_meta_first`/`spa_meta_region_is_valid`, static-inline helpers absent
  from ubuntu-22.04's `libspa-0.2-dev`; separately, `cros-libva`'s AV1 encode
  struct bindings need fields ubuntu-22.04's libva 2.14 doesn't have.
  `native-assets-linux`/`bindings-tests-linux` now pin **`ubuntu-24.04`**
  instead (still pinned, not `ubuntu-latest` — the "don't let the runner
  image silently drift the floor" principle is unchanged, just the specific
  pin). **glibc floor is now ≥ 2.39, not ≥ 2.35** — the PyPI wheel tag
  changed to `manylinux_2_39_x86_64` accordingly. The § Linux glibc baseline
  design section below is left as originally written for the historical
  record of what was first decided and why; treat this note as the
  authoritative correction.
- Android (ADR-0025) remains explicitly out of scope, per this ADR's
  original decision — not touched by the v0.1.7 work.

## Context

`.github/workflows/release.yml` builds native binary assets on exactly one
runner today: the `native-assets` job (`windows-latest`), targeting
`x86_64-pc-windows-gnu` only (MinGW-w64/ucrt64), via
`bun tools/scripts/copy-native-dlls.ts --release`. Its output — a single
`mediaway_ffi.dll` — is staged into all four binding trees
(`bindings/nodejs/packages/ffi/native`, `bindings/python/mediaway/_native`,
`bindings/csharp/runtime/win-x64/native`, `bindings/native/runtime/win-x64`)
and consumed unchanged by every downstream publish job (`npm`, `nuget`,
`pypi`, `native`/CPack). There is no Linux `.so` or macOS `.dylib` build step
anywhere in `release.yml`. **Every published `@mediaway/*` npm package,
`Mediaway.*` NuGet package, the `mediaway` PyPI wheel, and the CPack C
archive are Windows-only today.** A non-Windows user installing any of them
gets no working native library (source build aside).

This is distinct from `.github/workflows/ci.yml`, which already compiles/
lints/tests on Linux (`ubuntu-latest`, part of the `rust` matrix), macOS
(`macos-14`, jobs `apple-macos`/`apple-ios`), and Android (`ubuntu-latest` +
NDK, job `android`) — but `ci.yml` only proves the Rust workspace compiles;
none of that reaches the shipped binding packages, because `release.yml`
never runs those toolchains. `mediaway-decoder` was until this session
missing from the `android`/`apple-macos`/`apple-ios` CI jobs entirely (only
`mediaway-encoder`/`mediaway-device` were compiled there) — now fixed, so
those three jobs at least compile the full Rust surface for those targets.
Still CI-only; no release packaging.

**Facts gathered before deciding:**

- `mediaway-ffi` is **one consolidated crate** (ADR-0021) producing one
  cdylib (`mediaway_ffi.{dll,so,dylib}`) that covers container + device +
  pipeline behind a single header set. There is no per-capability DLL split
  to multiply across platforms — one artifact per (OS, arch) pair, mirroring
  today's one Windows DLL.
- Platform backends are `#[cfg]`-gated modules inside `mediaway-encoder`/
  `-decoder`/`-device` (ADR-0021), not separate crates: Linux already has
  VA-API (encoder/decoder, via `cros-libva`) and PipeWire (device) backends;
  macOS/iOS already have VideoToolbox/AVFoundation backends, hardware-verified
  per those crates' own ADRs (per project memory: Apple encode/decode arc
  shipped and merged 2026-08-19). `docs/roadmap.md` still lists **"Linux
  Hardware Verification"** as an open item — the Linux backends compile/lint/
  test-verified via WSL2, but have not been verified against a real Linux
  desktop's actual camera/screen/GPU. This maturity gap is real and predates
  this ADR; multi-platform binding packaging does not close it, and this
  ADR's Stage 1 scope note (below) says so explicitly rather than implying
  parity.
- The Node and Python binding loaders **already anticipate Linux** at the
  source level: `bindings/nodejs/packages/ffi/src/index.ts`'s
  `libraryFilename()` and `bindings/python/mediaway/_ffi.py`'s
  `_library_filename()` both switch on `win32`/Linux and resolve
  `libmediaway_ffi.so`, but explicitly `throw`/`raise` for any other platform
  with the comment *"macOS support is not claimed here since it has never
  been built or run."* Extending these two functions with a `darwin`/
  `Darwin` branch (`libmediaway_ffi.dylib`) is a small, well-contained change
  — the load-path *logic* is not the gap; the **release-time build/stage
  step** is.
- `bindings/csharp/src/Directory.Build.targets` already stages into
  `runtimes/win-x64/native` — **the standard NuGet RID-native-asset
  convention**, just hardcoded to one RID today
  (`MediawayNativeDir` defaults to `..\runtime\win-x64\native`). Multi-RID
  support is additive to an already-correct shape, not a restructuring.
- `bindings/cpp/CMakeLists.txt` already has a `MEDIAWAY_NATIVE_DIR_OVERRIDE`
  hook and a parameterized `CPACK_PACKAGE_FILE_NAME` — adding per-OS archive
  jobs is close to free.
- `bindings/python/setup.py` already forces a non-default wheel platform tag
  (`bdist_wheel.plat_name = "win_amd64"`) specifically so PyPI refuses to
  serve the Windows wheel to non-Windows interpreters — this is direct
  precedent that the "pick one glibc/macOS floor and tag honestly" pattern
  this ADR proposes for Linux/macOS wheels is already this codebase's own
  convention, not a new idea.
- `bindings/nodejs/packages/ffi/package.json` bundles the DLL **directly
  inside** `@mediaway/ffi` (`files: ["dist", "native"]`) — there is no
  per-platform package split and no `optionalDependencies` mechanism yet.
- `docs/roadmap.md` / `docs/ai/wiki/platform/order.md` already set the
  workspace's platform order: **Windows → Web → Linux → Other (Apple,
  Android, …)**. This ADR's Linux-then-macOS rollout order for *binding
  packaging* matches that existing, already-decided policy rather than
  introducing a new one.
- PyPI's wheel-filename validation **rejects bare `linux_*` platform tags**
  outright; only `manylinux*`/`musllinux*`-tagged wheels are accepted for
  Linux. Any Linux wheel plan must produce a `manylinux_X_Y` tag (PEP 600
  allows arbitrary `X_Y`, not only the pre-built `manylinux_2_28` etc. Docker
  profiles), or PyPI refuses the upload.
- GitHub-hosted `macos-14` runners are real Apple Silicon hardware (not an
  emulator) — VideoToolbox hardware encode/decode can genuinely run there,
  matching this workspace's existing Apple hardware verification. GitHub-
  hosted `ubuntu-latest` runners have **no GPU** — VA-API hardware
  encode/decode cannot be exercised there, matching the roadmap's existing
  "Linux Hardware Verification" gap; only container (sans-io) round-trips
  are realistically CI-verifiable on Linux runners today.

## Decision

> Extend native binding distribution beyond Windows in two separately
> landed, sequenced stages: **Stage 1 — Linux x86_64**, **Stage 2 — macOS
> x86_64 + arm64 (separate artifacts, not a universal binary)**. Android
> binding-package distribution is **explicitly out of scope** for this ADR.

### 1. Platforms, order, and explicit scope

| Stage | Platform/arch | Rationale |
|---|---|---|
| 1 | Linux x86_64 (`x86_64-unknown-linux-gnu`) | Cheapest GH Actions runner class; `ci.yml`'s `rust` matrix already proves the Rust workspace compiles+tests there today; matches the workspace's existing Windows→Web→Linux platform order. |
| 2 | macOS x86_64 + arm64 (`x86_64-apple-darwin`, `aarch64-apple-darwin`) | Real hardware-verified Apple backends exist (VideoToolbox); more expensive GH Actions runner class; a real, undecided code-signing/Gatekeeper risk (see § Deferred) that Linux does not have — sequenced after Linux deliberately, not bundled into one PR. |
| Out of scope | Android | Android consumers reach Mediaway through the NDK `.so` directly via Gradle/AAR — **not** through npm/NuGet/PyPI/CPack, the four ecosystems this ADR is about. Folding Android into this ADR would conflate two unrelated packaging problems (binding-package distribution vs. Gradle/Maven artifact publishing) under one decision. A dedicated future ADR should design the AAR/Maven shape when a concrete Android-binding consumer exists — this is a deliberate call, not a silent omission. |
| Out of scope | Linux/Windows ARM64, musl (Alpine), universal macOS binary, iOS binding package | No concrete consumer yet; each is its own future decision once demand exists (mirrors ADR-0017's "no `netstandard2.1` until a concrete consumer" precedent). |

**Linux glibc baseline**: build on a **pinned `ubuntu-22.04`** GH-hosted
runner (not `ubuntu-latest`, whose underlying image version drifts over
time and would silently move the floor), giving a **glibc ≥ 2.35** floor —
roughly Ubuntu 22.04+, Debian 12+, Fedora 36+. This is a documented,
honest floor, not a `manylinux_2_28`/`manylinux2014`-style broad-compat
claim. Rejected the alternative of building inside a `manylinux_2_28`
Docker container for a wider glibc floor: `mediaway-device`'s Linux backend
already depends on PipeWire, which itself effectively requires a modern
desktop distro (PipeWire is not a meaningful dependency on the
CentOS/RHEL-8-era systems `manylinux_2_28` targets) — chasing a glibc floor
wider than the underlying capability's own realistic deployment target is
unjustified complexity for v1 (`docker`-in-CI, PipeWire dev headers not
reliably available in a minimal manylinux base image). Revisit only if a
real compatibility bug report shows an actual consumer on an
glibc-2.35-incompatible-but-otherwise-supported distro.

**macOS deployment target**: `MACOSX_DEPLOYMENT_TARGET=11.0` (Big Sur — the
first macOS release with native Apple Silicon support) for **both**
architectures, keeping one uniform floor across `x86_64`/`arm64` instead of
two different minimums. Adjustable later if a concrete pre-Big-Sur x86_64
consumer appears (same "no speculative wider floor" reasoning as the Linux
glibc call above).

### 2. Per-ecosystem packaging shape

**NuGet** — add RID-specific `runtimes/<rid>/native/` folders inside the
**existing** four packages (`Mediaway.Common` has no native asset;
`Mediaway.Container`/`Mediaway.Device*`/`Mediaway.Pipeline` do). No package
split. This is additive to `Directory.Build.targets`' already-correct
convention: `MediawayNativeDir` becomes RID-parameterized (staged at
`bindings/csharp/runtime/<rid>/native/` per platform build job) and the
`<ItemGroup>` loops over every RID directory that exists rather than one
hardcoded path. RIDs: `win-x64` (already shipping), `linux-x64` (Stage 1),
`osx-x64` + `osx-arm64` (Stage 2). Rejected splitting into per-RID packages
(`Mediaway.Container.win-x64`, …) — NuGet's RID-folder convention exists
precisely so one package works across RIDs via the SDK's own runtime
resolution; a package-per-RID split would contradict that convention and
break `dotnet publish -r <rid>`'s expected behavior for consumers.

**npm** — restructure `@mediaway/ffi` from "bundles the Windows DLL
directly" into a **thin JS-only loader package** with
`optionalDependencies` on new per-platform packages:
`@mediaway/ffi-win32-x64`, `@mediaway/ffi-linux-x64` (Stage 1), then
`@mediaway/ffi-darwin-x64` + `@mediaway/ffi-darwin-arm64` (Stage 2). Each
platform package sets `os`/`cpu` fields so npm installs only the one
matching the consumer's machine; `@mediaway/ffi`'s loader resolves whichever
one actually installed (`require.resolve('@mediaway/ffi-<platform>/...')`-
style, replacing today's single `<package>/native` directory scan). This
mirrors the established pattern used by `esbuild`/`@swc/core`/`sharp` for
native npm packages, and keeps a consumer who only ever runs on Windows from
downloading Linux/macOS binaries too — a real bandwidth/disk concern given
`mediaway_ffi` bundles container+device+pipeline in one cdylib (this
package's own description already claims "lightweight"). The four
downstream capability packages (`@mediaway/container`/`device`/`encoder`/
`decoder`) need **no change** — they only depend on `@mediaway/ffi`, never
touch the native binary path directly. Rejected the simpler alternative
(bundle all platforms' binaries inside `@mediaway/ffi` itself, pick the
right one at runtime): mechanically simpler (no new packages, no
`optionalDependencies` wiring) but forces every install, on every OS, to
download every platform's binary — real, avoidable waste this ADR should
not introduce given the project's own "lightweight" framing.

**PyPI** — platform-tagged wheels, one wheel per (OS, arch), via the
**existing** `setup.py` `bdist_wheel.plat_name` forcing mechanism (already
used for `win_amd64`) rather than adopting `cibuildwheel`/`maturin`:
`win_amd64` (shipping), `manylinux_2_35_x86_64` (Stage 1 — PEP 600 allows
this exact-floor tag; matches the Linux glibc baseline decided above
honestly instead of claiming a looser `manylinux_2_28`/`manylinux2014` this
build does not actually satisfy), `macosx_11_0_x86_64` +
`macosx_11_0_arm64` (Stage 2, matching the macOS deployment target decided
above). Staying with the hand-rolled `plat_name` + Bun/TS build script
approach (not `cibuildwheel`) matches this repo's stated tooling preference
(`docs/conventions/scripts.md` — Bun/TS for light repo scripts) and this is
not a compiled-CPython-extension wheel (pure-Python ctypes package bundling
a prebuilt cdylib) — `cibuildwheel`'s main value (matrix-building actual
CPython C-extensions across ABI versions) does not apply here. Rejected
shipping a source-only sdist that builds Rust locally at `pip install`
time: would force every Python consumer to have a Rust toolchain **and**
the system dev packages (`libpipewire-0.3-dev`, `libva-dev` on Linux) just
to install a Python package — defeats the point of prebuilt wheels. An
sdist fallback for platforms with no prebuilt wheel may be added later as a
secondary path, not the primary one this ADR designs.

**CPack** — mechanical, closest to free given the existing
`MEDIAWAY_NATIVE_DIR_OVERRIDE` hook: one archive per (OS, arch) from a
dedicated CMake configure+build+cpack invocation per platform job,
`CPACK_PACKAGE_FILE_NAME` parameterized instead of the current hardcoded
`Mediaway-<version>-win64` (→ `Mediaway-<version>-linux-x64`,
`Mediaway-<version>-macos-x64`, `Mediaway-<version>-macos-arm64`), each
uploaded as its own GitHub release asset alongside the existing win64 zip/
tar.gz.

### 3. CI/CD shape

Two new `native-assets-*` jobs mirror the existing `native-assets` job,
each gated the same way (`needs: [version, crates]`, so a crates.io
publish failure still aborts the whole pipeline before any binding
artifact builds):

- `native-assets-linux` (`ubuntu-22.04`): install `libpipewire-0.3-dev`
  `libva-dev` (same packages `ci.yml`'s `crates`/`rust` jobs already
  install for Linux verification builds), `cargo build --release --target
  x86_64-unknown-linux-gnu -p mediaway-ffi`, stage `libmediaway_ffi.so` into
  the same four binding trees under a `linux-x64` subpath, upload as
  `native-dlls-linux`.
- `native-assets-macos` (`macos-14`): `rustup target add
  x86_64-apple-darwin` (native target, `aarch64-apple-darwin`, already
  present), build both, stage both `.dylib`s under `osx-x64`/`osx-arm64`
  subpaths, upload as `native-dlls-macos`.

Every publish job (`npm`, `nuget`, `pypi`, `native`) adds
`native-assets-linux`/`native-assets-macos` to its `needs:` and downloads
all three artifacts before packing, instead of the current single
`native-dlls` download.

`bindings-tests` (the RC gate) gets two mirror jobs,
`bindings-tests-linux` (`ubuntu-22.04`) and `bindings-tests-macos`
(`macos-14`), each downloading only its own platform's artifact:

- Both run the **container round-trip tests** (C/C#/Python/Node mux→demux)
  — sans-io, hardware-free, and the only Linux-side scope this ADR can
  honestly claim is CI-verified end-to-end.
- `bindings-tests-macos` additionally runs the **real hardware** pipeline
  encode/decode round-trip (the same class of test `bindings-tests`
  already runs for WMF/Opus on Windows), since `macos-14` is real Apple
  Silicon hardware with a verified VideoToolbox backend.
- `bindings-tests-linux` does **not** attempt a hardware pipeline
  round-trip — GitHub-hosted Linux runners have no GPU, and
  `docs/roadmap.md`'s "Linux Hardware Verification" gap is unresolved by
  this ADR. This asymmetry is deliberate and should be stated plainly in
  each package's release notes/description for the Linux artifacts
  (container: verified; hardware encode/decode/device-capture: compiled,
  not yet real-desktop-verified).

Every publish job's `needs:` also grows to include the new
`bindings-tests-*` jobs, keeping the existing "RC gate blocks every
publish" invariant for the new platforms.

The `crates` job (crates.io) is **unaffected** — Rust crates publish as
source, not platform-specific binaries; this ADR only concerns the four
binding-package pipelines downstream of `native-assets*`.

### 4. Rollout sequencing

Not a single big-bang change — each stage is its own PR against `main`,
run through the real `release.yml` dry-run path before merging:

1. **This ADR** (design review, no code).
2. **Stage 1 PR** — Linux x86_64: `native-assets-linux` +
   `bindings-tests-linux` jobs; NuGet `linux-x64` RID; npm
   `@mediaway/ffi-win32-x64`/`-linux-x64` split (the npm restructuring
   happens once, here, not deferred to Stage 2); PyPI
   `manylinux_2_35_x86_64` wheel; CPack `linux-x64` archive. Verified via
   `workflow_dispatch` with `dry_run: true` before a real release branch
   push.
3. **Stage 2 PR** — macOS x86_64 + arm64: `native-assets-macos` +
   `bindings-tests-macos` jobs; NuGet `osx-x64`/`osx-arm64` RIDs; npm
   `@mediaway/ffi-darwin-x64`/`-darwin-arm64` packages (same
   `optionalDependencies` mechanism Stage 1 already built); PyPI
   `macosx_11_0_x86_64`/`macosx_11_0_arm64` wheels; CPack
   `macos-x64`/`macos-arm64` archives. Blocked on resolving (or explicitly
   re-confirming and documenting) the code-signing/Gatekeeper open question
   below with a real-device smoke test — not shipped on faith.
4. **Future, separate ADR(s)**: Android AAR/Maven distribution;
   Linux/Windows ARM64; musl; universal macOS binary if ever requested;
   iOS binding package.

### 5. Honest scope for this release

This ADR changes no code and ships no packages. Until Stage 1 actually
merges and a release goes out with a real Linux artifact,
`docs/spec/status.md` and `RELEASE_NOTES.md` must keep stating that binding
packages are Windows-only — this ADR does not imply otherwise, and no file
outside `docs/adr/` is touched by it.

## Alternatives Considered

| Alternative | Why not |
|---|---|
| Bundle Android into this same ADR | Android reaches Mediaway via NDK `.so` + Gradle/AAR, not npm/NuGet/PyPI/CPack — a genuinely different packaging problem; conflating it here would blur both decisions. Deliberate exclusion, tracked as a future ADR. |
| `manylinux_2_28` Docker container build for Linux | Wider glibc floor than needed given PipeWire's own modern-distro requirement; real CI friction (dev headers not reliably available in a minimal manylinux base image) for a compatibility floor no realistic consumer of the device-capture path would actually use. Revisit only on a real bug report. |
| Universal2 macOS binary (single fat `.dylib`) | Introduces a distribution shape with no equivalent on the other three platforms (Windows/Linux both ship one artifact per (OS, arch), not a fat multi-arch file); adds `lipo` tooling and universal-wheel-tag complexity for a benefit (one file instead of two) with no concrete consumer request yet. |
| npm: bundle all platform binaries inside `@mediaway/ffi` itself | Simpler wiring, but forces every install, on every OS, to download every platform's binary — contradicts the package's own "lightweight" framing; no ecosystem precedent among comparable native-npm packages does this. |
| PyPI: `cibuildwheel`/`maturin`-based build matrix | This is not a compiled CPython C-extension (pure-Python ctypes wrapper over a prebuilt cdylib) — `cibuildwheel`'s main value (per-CPython-ABI extension matrix) does not apply; the existing hand-rolled `plat_name`-forcing `setup.py` + Bun/TS script pattern already works and matches this repo's tooling convention. |
| PyPI: source-only sdist, build Rust at install time | Forces every Python consumer to have a Rust toolchain plus Linux system dev packages just to `pip install mediaway` — defeats the purpose of prebuilt wheels. Possible future fallback path, not primary. |
| NuGet: package-per-RID split | Contradicts NuGet's own RID-folder convention, which exists so one package resolves correctly across `dotnet publish -r <rid>` targets — the repo's existing `Directory.Build.targets` shape already follows this convention for the single RID it supports today. |
| Single big-bang PR for Linux + macOS together | Real CI cost and real risk (macOS runner cost, undecided code-signing question) against a working release pipeline; staging lets Linux's lower-risk change land and prove the multi-platform CI shape before macOS's higher-risk change is attempted. |

## Consequences

### Positive

- Closes a real, previously silent gap: non-Windows consumers of
  `@mediaway/*`, `Mediaway.*`, `mediaway` (PyPI), and the CPack C archive
  currently get no working native library at all.
- Every ecosystem's multi-platform mechanism is either already the correct
  convention with minimal extension (NuGet RID folders, CPack override hook,
  Node/Python loader platform switches) or a well-precedented pattern
  adopted deliberately (npm optionalDependencies split, PyPI platform-tagged
  wheels) — no ecosystem needs a bespoke, one-off design.
- Staged rollout (Linux first) reuses real `ci.yml` evidence the Rust
  workspace already compiles+tests on Linux, lowering Stage 1's risk before
  Stage 2's macOS-specific unknowns (runner cost, code signing) are taken
  on.
- The RC gate (`bindings-tests-*`) is extended per-platform with an honest
  scope per platform (container-only on Linux, container+hardware on
  macOS) rather than silently skipping validation for the new artifacts.

### Negative / Trade-offs

- Real CI cost increase: three `native-assets-*` and three
  `bindings-tests-*` jobs instead of one each; macOS GH Actions runner
  minutes are priced higher than Linux/Windows.
- `@mediaway/ffi`'s npm package shape changes from "bundles its own binary"
  to "thin loader + `optionalDependencies`" — a real restructuring of an
  already-published package, not purely additive (existing installs on
  `@mediaway/ffi@<version before this lands>` keep working; the shape
  change only affects new versions going forward).
- Linux packages ship with a real, documented capability gap versus
  Windows/macOS: hardware encode/decode/device-capture is compiled-in but
  not real-desktop-verified (pre-existing `docs/roadmap.md` gap, not newly
  introduced, but now visible to a wider audience once Linux packages
  publish).
- macOS Stage 2 carries a genuinely open risk (code signing/Gatekeeper —
  see below) that must be resolved with real-device testing, not assumed
  away, before it ships.
- Four more artifact classes (linux-x64, osx-x64, osx-arm64, plus the
  existing win-x64) to keep in sync across every publish job's `needs:`
  graph and every binding tree's staging paths — more moving parts for
  future maintainers to keep correct.

## Deferred / Open questions

- **macOS code signing / notarization / Gatekeeper.** This is not a signed
  installer or `.app` bundle (parity with today's unsigned Windows DLL), so
  full notarization may be unnecessary — but whether an **unsigned, ad-hoc
  or plain-unsigned `.dylib`** loaded via `dlopen`/`ctypes.CDLL`/koffi from
  a Node/Python/C# process is blocked by Gatekeeper once it carries a
  quarantine attribute (from an `npm install`/`pip install`/`nuget
  restore` download) is **not confirmed** by anything read for this ADR —
  it needs a real Mac, a real download-then-load smoke test, before Stage 2
  ships. Flagged explicitly as unresolved, not asserted safe.
- **Whether `manylinux_2_35_x86_64` is accepted by PyPI's upload
  validation as-is** (PEP 600 allows arbitrary `X_Y` values in principle;
  this ADR did not verify PyPI's current server-side acceptance behavior
  for a non-"named-profile" `manylinux_2_35` tag specifically — a real
  `twine upload`/dry-run check before Stage 1 ships is the confirmation
  step, not asserted here).
- **Android AAR/Maven binding-package distribution** — deliberately out of
  scope; its own future ADR.
- **Linux/Windows ARM64, musl (Alpine), universal macOS binary, iOS binding
  package** — no concrete consumer yet; each is its own future decision.
- **`@mediaway/ffi`'s exact `optionalDependencies` resolution fallback
  behavior** (what happens on an unsupported platform/arch combination —
  a clear thrown error vs. a silent missing-module failure) needs to be
  designed as part of Stage 1's implementation, not fully specified here.

## References

- workflow: `.github/workflows/release.yml`, `.github/workflows/ci.yml`
- scripts: `tools/scripts/copy-native-dlls.ts`,
  `tools/scripts/package-csharp.ts`, `tools/scripts/build-python-package.ts`,
  `tools/scripts/build-node-packages.ts`
- crate: `crates/mediaway-ffi/Cargo.toml` (single consolidated cdylib)
- binding sources: `bindings/nodejs/packages/ffi/src/index.ts`,
  `bindings/python/mediaway/_ffi.py`, `bindings/python/setup.py`,
  `bindings/csharp/src/Directory.Build.targets`,
  `bindings/cpp/CMakeLists.txt`
- related ADR: [ADR-0003](0003-crate-packaging.md),
  [ADR-0004](0004-c-ffi.md), [ADR-0016](0016-cbindgen-ffi-headers.md),
  [ADR-0017](0017-csharp-binding-package-layout.md),
  [ADR-0020](0020-browser-wasm-npm-package.md),
  [ADR-0021](0021-workspace-consolidation.md)
- spec: [`docs/spec/c-ffi.md`](../spec/c-ffi.md),
  [`docs/spec/crate-packaging.md`](../spec/crate-packaging.md),
  [`docs/spec/status.md`](../spec/status.md)
- platform policy: [`docs/roadmap.md`](../roadmap.md) § Platform order,
  [`docs/ai/wiki/platform/order.md`](../ai/wiki/platform/order.md)

ADRs are written in **English**.
