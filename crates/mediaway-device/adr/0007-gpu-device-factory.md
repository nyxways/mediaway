# ADR-0007: GPU device factory — adapter enumeration + configurable DirectX11 device creation

- **Status**: Accepted (implemented, hardware-verified against real DXGI adapters)
- **Date**: 2026-08-11
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-device`

## Context

Every Zero-Copy capture/encode/decode path in this workspace that takes a
`GpuDeviceHandle` documents the same assumption: *the caller already owns a live
device* (`GpuDeviceHandle`'s own doc: "the caller owns the underlying device... this
crate never constructs or frees one"; `mediaway-ffi/adr/device/0003-gpu-handle-c-abi.md`
§4 enforces it — a `NONE`/malformed handle is rejected, not CPU-faked). Two prior ADRs
(`mediaway-ffi/adr/0001` §1, `mediaway-ffi/adr/device/0003-gpu-handle-c-abi.md` § Context)
both name "how does a caller *get* that device" as an explicitly deferred problem.

For a Rust caller embedding Mediaway inside an existing renderer/game engine, that
assumption holds for free — the host already has an `ID3D11Device` from its own
rendering pipeline, and handing it to Mediaway (rather than creating a redundant
second device) is the genuinely best-performance path. But every hardware-gated test
and example that does **not** have such a host (`crates/mediaway/tests/
screen_mic_av_smoke.rs`'s `open_shared_d3d11_device`, `mediaway-device`'s own
`dxgi_shared_tests.rs::create_test_device`, `mediaway-ffi/tests/gpu_write_frame_smoke.rs`)
hand-rolls the exact same raw `D3D11CreateDevice` call independently, each hardcoding
`D3D_DRIVER_TYPE_HARDWARE` + the default adapter — there is no shared, reusable,
public function for it anywhere, Rust or FFI. Callers who don't already have a device
(every FFI/binding consumer today — Node.js, Python, C, C++, and C#'s own examples)
have no path forward at all short of reimplementing raw Win32 device creation
themselves. C#'s test suite proves this works (`CaptureTests.cs`'s
`NativeD3D11.CreateHardwareDevice`), but its own shipped `ScreenRecord.cs` example
stubs the same gap with `NotImplementedException` — the capability has never actually
been reusable, only duplicated per call site.

A second, related gap: nothing in this workspace enumerates *which* GPU adapters
exist. `windows_desktop::dxgi::enumerate_outputs` walks `IDXGIFactory1::EnumAdapters1`
internally, but only to reach *monitor outputs* — adapter identity (name, vendor,
dedicated VRAM, hardware vs. WARP) is discarded, not surfaced.

## Decision

> Add a first-class Rust API to `mediaway-device` — not FFI-only — for listing GPU
> adapters and creating a configurable `ID3D11Device` against one of them, so every
> caller without a pre-existing device (Rust or FFI) has exactly one place to get one.
> `mediaway-ffi` wraps this with a thin C ABI in a follow-up (Phase 2); this ADR covers
> the Rust surface only.

```rust
// mediaway-device/src/windows/gpu.rs

/// One physical GPU adapter DXGI reports.
pub struct GpuAdapterInfo {
    pub index: u32,
    pub name: String,
    pub vendor_id: u32,
    pub device_id: u32,
    pub dedicated_video_memory: u64,
    pub is_hardware: bool, // false = WARP/software adapter
}

/// List every adapter DXGI can see, in enumeration order. `index` is stable
/// for a `GpuAdapterSelect::Index` call in the same process run (DXGI's own
/// enumeration order does not change without a topology change).
pub fn enumerate_gpu_adapters() -> Result<Vec<GpuAdapterInfo>, CaptureError>;

/// Which adapter to open a device against — auto or explicit.
pub enum GpuAdapterSelect {
    /// First hardware adapter DXGI reports (skips WARP/software) — same
    /// selection `D3D11CreateDevice(None, D3D_DRIVER_TYPE_HARDWARE, ..)`
    /// already made implicitly at every existing call site.
    Default,
    /// An `index` from `enumerate_gpu_adapters()`.
    Index(u32),
}

/// Device-creation knobs this workspace's capture/encode/decode paths
/// actually use — not every `D3D11_CREATE_DEVICE_FLAG` bit.
pub struct GpuDeviceOptions {
    pub adapter: GpuAdapterSelect,
    /// `D3D11_CREATE_DEVICE_VIDEO_SUPPORT` — required by every capture/
    /// encode/decode path that takes this device; default `true`.
    pub video_support: bool,
    /// `D3D11_CREATE_DEVICE_DEBUG` — real driver-side cost, opt-in only;
    /// default `false`.
    pub debug_layer: bool,
}
// Default::default() == { adapter: Default, video_support: true, debug_layer: false }
// — the exact behavior every existing hand-rolled call site already has today.

/// An owned DirectX11 device. Drop releases the underlying COM object —
/// `handle()`'s `GpuDeviceHandle` bits are only valid while this `GpuDevice`
/// (or another owner of the same `ID3D11Device`) is alive, the same
/// caller-tracked-lifetime contract `GpuDeviceHandle` already documents.
pub struct GpuDevice { /* ID3D11Device */ }

impl GpuDevice {
    pub fn create(options: GpuDeviceOptions) -> Result<Self, CaptureError>;
    #[must_use]
    pub fn handle(&self) -> GpuDeviceHandle;
}
```

Reuses `CaptureError` (already `#[non_exhaustive]`, already this crate's one error
type for backend/hardware failures) rather than inventing a parallel error enum for
one more Windows-backend concern — `Backend` for `D3D11CreateDevice`/DXGI factory
failures, `InvalidInput` for an out-of-range `GpuAdapterSelect::Index`.

Lives in `crate::windows` (sibling to `windows::capabilities`/`windows::enumeration`),
**not** `windows_desktop` — this is not a desktop-capture concern. Camera and Audio
backends don't need a GPU device; encode/decode do (via `mediaway-ffi` handing the
resulting handle to `AutoVideoEncodeConfig`/decode configs, which already accept an
externally-supplied `GpuDeviceHandle` — no change needed on that side), but
`mediaway-encoder`/`mediaway-decoder` do not depend on `mediaway-device`, so this
function cannot live there either without a new cross-crate dependency. `mediaway-ffi`
already depends on both `mediaway-device` and `mediaway-encoder`/`-decoder`, so it is
the natural place to bridge a `mediaway-device`-created handle into an encoder/decoder
config — Phase 2, not this ADR.

### Ownership — why a new owning type, not just more `NativeHandle`s

`NativeHandle`/`GpuDeviceHandle` are deliberately non-owning value types (raw pointer
bits) — every existing call site keeps the real `ID3D11Device` COM object alive
separately in scope (e.g. `screen_mic_av_smoke.rs`'s `(_device, device_handle)` tuple)
specifically so a caller-owned device's lifetime story stays caller-owned even when
Mediaway didn't create it. Now that Mediaway *can* create one, something has to own
the resulting COM refcount — `GpuDevice` is that something. It is intentionally not
`Clone` (a second owner would need its own refcount bump, not a bit copy); `.handle()`
borrows `&self` and returns the value-type bits, matching the existing pattern exactly.

## Alternatives Considered

| Alternative | Why not |
|---|---|
| Put this in `mediaway-common` (already shared by every crate that touches `GpuDeviceHandle`) | `mediaway-common` is deliberately OS-backend-free (`forbid(unsafe_code)`, no `windows`/`ash`/etc. dependency) — a real `D3D11CreateDevice` call would break that boundary for the one crate every other crate depends on. |
| FFI-only helper (`mediaway-ffi` calls raw `D3D11CreateDevice` itself, no Rust-level API) | Fails "low-level APIs stay public" (`docs/spec/api-layers.md`) — leaves Rust callers without a pre-existing device (a real, if smaller, population: standalone tools, tests, examples) with the exact same unmet gap FFI callers have today. |
| A full adapter-agnostic abstraction (`GpuBackend` trait covering D3D11/D3D12/Vulkan/Metal device creation uniformly) | No second backend (Vulkan/Metal device creation) is in scope yet — `mediaway-encoder`'s Vulkan backend already creates its own `vulkanalia` device internally and does not need this factory. Speculative generalization for platforms with zero concrete demand today; add a real trait when a second platform actually needs one. |
| Return the raw `windows::Win32::Graphics::Direct3D11::ID3D11Device` type directly from `enumerate_gpu_adapters`/`create` instead of a facade-owned `GpuDevice` wrapper | Would leak a `windows`-crate type through this crate's public API on non-Windows-conditional code paths, and gives non-Windows builds nothing to `#[cfg(not(windows))]`-stub against with the same shape — `GpuDevice` keeps the public surface platform-uniform (host-stub returns `CaptureError::Unsupported`) the same way every other capability in this crate already does. |

## Consequences

### Positive

- One reusable, tested, public function replaces N hand-rolled `D3D11CreateDevice`
  call sites (already 3+ in this workspace) with the exact same default behavior they
  already had, plus adapter listing/selection and debug-layer opt-in none of them had.
- FFI/binding callers (Node.js, Python, C, C++) get a real path to a working
  `GpuDeviceHandle` for the first time — the root blocker named in the chat history
  motivating this ADR (screen capture and the capture-encode bridge are otherwise
  unreachable from every non-Rust binding, C# included in practice).
- `enumerate_gpu_adapters` + `GpuAdapterSelect::Index` gives multi-GPU machines a real
  choice instead of always silently taking whatever `D3D_DRIVER_TYPE_HARDWARE`'s
  default adapter resolution picks.

### Negative / Trade-offs

- `GpuDevice::create` opens a **new**, unshared device — on a machine already running
  a host renderer with its own device, this is a second device instance, not a reuse
  of the first. This ADR does not solve "share the host application's existing
  device" (that story is what a Rust caller passing their own `GpuDeviceHandle`
  already covers, unchanged); it solves "no existing device to share."
- Windows/DirectX11-only. Vulkan/Metal/WebGPU equivalents are out of scope (see
  Alternatives) — a caller on Linux/macOS/Web still has no factory and must keep
  bringing their own device via the platform's own SDK.

## Deferred (out of scope for this ADR)

- `mediaway-ffi` C ABI wrapping this Rust API (Phase 2 of the same effort).
- Vulkan/Metal/WebGPU device factories.
- Multi-adapter (mGPU) explicit work distribution — `GpuAdapterSelect::Index` picks
  one device for one caller; nothing here coordinates multiple devices together.
- A `create_directx12_device` counterpart — `mediaway-encoder`'s D3D12 backend opens
  its own device internally today (not caller-supplied); unify only if a real need
  for a caller-supplied D3D12 device appears.

## References

- `mediaway-common/src/gpu.rs` — `GpuDeviceHandle`/`NativeHandle`, the non-owning
  value types this ADR's `GpuDevice::handle()` produces
- `mediaway-ffi/adr/device/0003-gpu-handle-c-abi.md` — the C ABI shape this ADR's
  Phase 2 follow-up wraps; §4's "no CPU fallback, handle enforced" contract this ADR
  exists to let callers actually satisfy
  `mediaway-ffi/adr/0001-auto-encode-c-abi.md` — §1, the encode-side "GPU handle
  crossing a C boundary is unsolved" deferral this ADR also addresses
- `crates/mediaway/tests/screen_mic_av_smoke.rs`'s `open_shared_d3d11_device` —
  the hand-rolled pattern this ADR generalizes and replaces
- `crates/mediaway-device/src/windows_desktop/dxgi_shared_tests.rs`'s
  `create_test_device` — a second, independent hand-rolled instance of the same call
- `docs/spec/gpu-interop.md` — Zero-Copy / bring-your-own-device design this ADR does
  not change, only extends for callers with nothing to bring

ADRs are **English**. Numbering is local to this `adr/` folder.
