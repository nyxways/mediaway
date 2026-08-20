# ADR-0004: `AutoEncoder::open` — wire Linux/Apple `ZeroCopyGpu` into the auto chain

- **Status**: Accepted — hardware-verified (Linux path not verifiable, no Linux GPU here)
- **Date**: 2026-08-20
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway` (`platform` module)

## Context

`AutoEncoder::open` (`src/platform.rs`) dispatches per OS. The Windows branch
(`mediaway_encoder::windows::auto::AutoVideoEncoder::open`) already ranks `ZeroCopy` →
`GpuCopy` → `CpuUpload` → `Software` based on `config.gpu_device` (ADR-0004 in
`mediaway-encoder`). The Linux and macOS/iOS branches instead **hardcode**
`VideoInputPreference::CpuUploadOk`, unconditionally, regardless of `config.gpu_device`:

```rust
// current Linux/Apple branches, both shaped identically
let low_level = config.to_low_level(VideoInputPreference::CpuUploadOk, config.gpu_device);
```

This is stale relative to what the backend crates actually implement:

- `mediaway-encoder::linux`'s `VaapiVideoEncoder` has a real `VideoInputPreference::ZeroCopyGpu`
  path (DMA-BUF import via `vaCreateSurfaces`, `adr/linux/0006-vaapi-dmabuf-zero-copy-input.md`)
  — a caller-supplied `GpuBufferHandle::DmaBuf` surface is imported and encoded with no CPU
  upload at all.
- `mediaway-encoder::apple`'s `VideoToolboxVideoEncoder` has a real `VideoInputPreference::
  ZeroCopyGpu` path (`GpuBufferHandle::Metal` `CVPixelBuffer` borrowed directly for
  `VTCompressionSession::encode_frame`, `adr/apple/0003-videotoolbox-metal-zero-copy-encode.md`).

Neither backend's `open()` actually reads `config.gpu_device`'s *content* — both branch purely
on `config.input` (confirmed directly from `linux/vaapi/video.rs:104-111` and
`apple/videotoolbox.rs`'s own module doc). `gpu_device`'s role here is the same signal the
Windows chain already uses it for: "the caller has GPU-resident frames to push," not a device
handle either backend needs to open a session. So the fix mirrors the Windows chain's shape
without needing any new taxonomy — just correctly wiring already-implemented capability.

## Decision

> For the `target_os = "linux"` and `any(macos, ios)` branches of `AutoEncoder::open`: when
> `config.gpu_device` is `Some(_)`, try `VideoInputPreference::ZeroCopyGpu` first; on failure
> (or when `gpu_device` is `None`), fall back to `VideoInputPreference::CpuUploadOk` — same
> two-tier shape the Windows chain already establishes, just without a `GpuCopy` middle tier
> (neither backend has one).

```rust
#[cfg(target_os = "linux")]
{
    use mediaway_encoder::VideoInputPreference;
    use mediaway_encoder::linux::LinuxVideoEncoder;
    if config.gpu_device.is_some() {
        let low = config.to_low_level(VideoInputPreference::ZeroCopyGpu, config.gpu_device);
        if let Ok(enc) = LinuxVideoEncoder::open(&low) {
            return Ok(Box::new(enc));
        }
    }
    let low = config.to_low_level(VideoInputPreference::CpuUploadOk, config.gpu_device);
    let enc = LinuxVideoEncoder::open(&low)?;
    Ok(Box::new(enc))
}
```

Same shape for the Apple branch, substituting `AppleVideoEncoder`. `gpu_device`'s exact
`GpuDeviceHandle` variant is not inspected — any `Some(_)` is read as "caller intends to push
GPU frames," matching how neither backend's `open()` reads the handle's contents either. A
caller passing a mismatched variant (e.g. `GpuDeviceHandle::Metal` on Linux) simply fails the
`ZeroCopyGpu` open attempt and falls through to `CpuUploadOk` — no special-casing needed.

### Why not gate on the specific `GpuDeviceHandle` variant

The Windows chain gates on `GpuDeviceHandle::DirectX11`/`DirectX12` specifically because it
must route to *different* bridging code per variant (native DX11 vs. the D3D12 `GpuCopy`
bridge). Linux/Apple have exactly one GPU input path each — there is nothing to route between,
so the variant's identity is irrelevant; only "is there one at all" matters. Checking a specific
variant here would just be a redundant, narrower version of the same `is_some()` check with no
behavioral difference (a wrong variant still fails to open and falls through either way).

## Alternatives Considered

| Alternative | Why not |
|---|---|
| Always try `ZeroCopyGpu` first regardless of `gpu_device` | Both backends' `push_frame` validates the pushed frame's `VideoFrameStorage` against the `open()`-time preference and rejects a mismatch (`linux/vaapi/video.rs:427-434`) — a caller who only ever pushes CPU frames would have their encoder opened in the wrong mode and every `push_frame` would fail. `gpu_device: Some(_)` is the caller's only signal of intent; skipping it breaks the common CPU-frame case. |
| Add `Backend`/`EncodePathClass`-level ranking (mirror Windows' `try_gpu_copy`-style dispatch) | No `GpuCopy` tier exists on either backend — there is nothing to rank beyond the two-tier `ZeroCopy`/`CpuUpload` choice; adding the extra machinery would be unused abstraction. |
| Gate on the specific `GpuDeviceHandle::DmaBuf`/`Metal`-equivalent variant | No such device-level variant exists for Linux (`GpuBufferHandle::DmaBuf` is a per-frame buffer handle, not a `GpuDeviceHandle` device handle) — see § Why not gate on the specific variant. |

## Consequences

### Positive

- `AutoEncoder::open` now actually reaches the Zero-Copy GPU paths `mediaway-encoder::linux`/
  `::apple` already ship, closing a real gap where a fully-implemented capability was
  unreachable through the cross-platform facade.
- No new types, no `Backend`/`EncodePathClass` changes — minimal, behavior-only diff.
- Existing CPU-frame callers (`gpu_device: None`) see no behavior change at all.

### Negative / Trade-offs

- Not hardware-verifiable on this session's machine for Linux (no VA-API device) or Apple (no
  macOS/iOS host) — same "zero compile/hardware verification as authored" caveat the backend
  crates themselves already carry for these paths (see their own ADRs).
- `encoder_support`'s probe function is unchanged — it still only probes `Backend::Os`'s
  `CpuUpload`-shaped path on Linux/Apple (a `gpu_device`-bearing probe would need a real
  `GpuBufferHandle` to construct, out of scope for a capability-listing probe). A future pass
  could add a second probed row once a synthetic DMA-BUF/`CVPixelBuffer` fixture exists for that
  purpose.

## References

- `mediaway-encoder` [ADR-0004: backend preference](../../mediaway-encoder/adr/0004-backend-preference.md)
  — the Windows chain this ADR mirrors for Linux/Apple.
- `mediaway-encoder-linux` `adr/linux/0006-vaapi-dmabuf-zero-copy-input.md` — the Zero-Copy
  capability this ADR wires in.
- `mediaway-encoder-apple` `adr/apple/0003-videotoolbox-metal-zero-copy-encode.md` — same, Apple.
- [`docs/ai/wiki/encode/backend-preference.md`](../../../docs/ai/wiki/encode/backend-preference.md).

ADRs are **English**. Numbering is local to this `adr/` folder.
