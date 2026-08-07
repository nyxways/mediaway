# Backend preference

Typed encode selection aligned with root README tables.  
ADR: [`0004-backend-preference.md`](../../../../crates/mediaway-encoder/adr/0004-backend-preference.md).

## Kinds (2026-07-31: flattened, see below)

```text
Backend: Os | Nvenc | QuickSync | Amf | Software
BackendSelection: Auto | AutoHardwareOnly | Explicit(Backend)
```

**`Os`** = OS-native graphics-interop path (one neutral tag; Media Foundation / VA-API /
`VideoToolbox` per platform crate) vs **vendor SDKs** = different backends, not filters
on one path. **Path class** (`zc`/`copy`/…, now `max_path_class: EncodePathClass`, an
`Ord` ceiling) is orthogonal to which `Backend` runs.

## Device select (orthogonal)

```text
DeviceSelect::Auto | GpuAdapter { id } | CpuTarget { … } | Compatible(…)
```

| Want | Set |
|------|-----|
| Default | `BackendSelection::Auto` + device Auto |
| GPU #2 | `Auto` + `GpuAdapter { … }` |
| CPU only (no GPU device set) | leave `gpu_device: None` (+ optional `CpuTarget`) |
| NUMA / affinity (Sw, upload) | `CpuTarget { numa / affinity }` |
| App’s D3D11 device | `Compatible(…)` |

**CPU-only note:** upload often still hits **HW encode on a GPU adapter** — may need `GpuAdapter` for silicon and `CpuTarget` for host work. Enumerate via platform helpers.

## Foreign intake

| Source | Intake | Label |
|--------|--------|-------|
| Same API + same device | Compatible open | `zc` |
| wgpu / Dawn / … | `mediaway::wgpu` (etc.) adapt | `zc` if shared |
| Cross-API / cross-GPU | Named bridge | `copy` / `readback` |
| Host CPU bytes | Os · Cpu | `upload` |

Prefer identity (app’s device) over re-pick. Map LUID/physdev when “chosen elsewhere”. Auto may compose a bridge only with an honest path class.

## Auto order

1. `Os` Zero-Copy (when frames already match that API)
2. Graphics copy / other GPU costs (labeled)
3. `Os` Cpu upload
4. Software if `max_path_class` allows
5. Vendor SDK (NVENC/QuickSync/Amf) **not** default #1 — control / cross-API tax; bench
   before claiming faster. Reachable via `BackendSelection::AutoHardwareOnly` (ranked
   ahead of step 3, never step 4) or `Explicit(Backend::Nvenc/QuickSync/Amf)`.

Same silicon often underlies WMF HW MFT and NVENC; a vendor SDK ≠ automatic win.

## Layers (wgpu-inspired, not copied)

| Layer | Mediaway |
|-------|----------|
| types | `CodecKind`, feeds, path class, preference enums |
| facade / auto | request / `open` + `path_class()` |
| session contract | push / pull / finish (not a thin GPU HAL) |
| platform | `*-windows` WMF/… native detail |

`mediaway::wgpu` = handle adapter only ([gpu-interop](../zero-copy/gpu-interop.md)).

## Vendor SDKs — implemented (2026-07-29), selection flattened (2026-07-31)

`mediaway-encoder::windows`'s `AutoVideoEncoder::open` dispatches on
`config.backend: BackendSelection`: `Explicit(Backend::Nvenc)` / `Explicit(Backend::QuickSync)`
each try exactly that vendor SDK (CPU-upload input only); `AutoHardwareOnly` tries both,
ranked ahead of `Os` CPU upload, never reached by plain `Auto`.
`Explicit(Backend::Software)` short-circuits to `mediaway-sw`'s software backend (AV1
only today). See [ADR-0004](../../../../crates/mediaway-encoder/adr/0004-backend-preference.md)'s
2026-07-31 addendum for why the old `EncodeMode`/`OsMode`/`GpuMode`/`FallbackPolicy`
shape was replaced (capability probe, `AutoHardwareOnly`, ceiling-not-bitflags).

- **NVENC** → `mediaway-encoder::nvenc`: real, hardware-verified H.264/HEVC/AV1
  CPU-upload encode on an RTX 4090 (`nvenc` crate, dynamically loads
  `nvEncodeAPI64.dll`). Zero-Copy input still deferred (D3D12 fence-based ZC is the
  hard remaining part). See adr/0001's 2026-07-29 addenda.
- **QuickSync** → `mediaway-encoder::quicksync`: real, hardware-verified H.264/HEVC
  CPU-upload encode on an Intel UHD 770 (new `vpl-sys` binding). AV1
  encode confirmed genuinely unsupported on this iGPU generation (`MFX_ERR_UNSUPPORTED`
  from `MFXVideoENCODE_Query`, not a bindings gap). See adr/0001's 2026-07-29 addenda.
- **AMF** → `mediaway-encoder::amf` adr/0001: **deferred**, not just
  unverified — `amf-rs` on crates.io is a *different*, unrelated GPL-3.0
  crate (real bindings are `shiguredo_amf`, Apache-2.0); `shiguredo_amf`
  needs Rust 1.93, this workspace pins 1.91 (hard MSRV block); no AMD GPU
  available either.
- **Vulkan Video** doesn't fit `GraphicsApi` or `VendorHw` cleanly — it's
  cross-vendor (unlike `VendorHw`) but packaged like a vendor crate
  (`mediaway-encoder::vulkan`, cross-OS unlike `GraphicsApi`'s current
  OS-crate-1:1 assumption). Taxonomy gap flagged, not resolved, by its own
  ADR-0001 — not decided by this doc either. H.264 + HEVC `VideoEncoder`
  (CPU-upload, all-intra) is real + hardware-verified (2026-07-29) — see
  [gpu-interop](../zero-copy/gpu-interop.md).
