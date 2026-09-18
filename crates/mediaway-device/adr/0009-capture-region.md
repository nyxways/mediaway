# ADR-0009: Capture a region of the surface, cropped on the GPU or by the pool

- **Status**: Accepted (WGC; other backends refuse)
- **Date**: 2026-09-19
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-device`

## Context

A QA recorder built on this crate (qarec) needs to "capture a specific rect of a specific
window". It is an explicit product requirement: an external harness asks for one panel of a
game window, not the whole of it. No backend could do that, and `mediaway`'s `FrameFilter`
rejects GPU frames outright, so the Zero-Copy path had no hook for it either.

ADR-0008 already established two things this builds on:

- What ends up **in the frames** is content, and a backend that cannot deliver requested
  content refuses at `open` instead of recording something else (`CursorCapture`).
- WGC **crops content to its frame pool, it does not scale it** (measured: a 2459×1341 pool
  on a 2560×1392 window gave 3 297 519 pixels identical to the full capture's top-left).

## Decision

> `DesktopVideoCaptureConfig::region: Option<CaptureRegion>` (`x`, `y`, `width`, `height` in
> the captured surface's pixels, origin top-left). A backend crops to it or refuses it.
> WGC crops. On WGC the pool is sized to the region's far corner, and an off-origin region
> is copied out of it on the GPU.

### WGC, concretely

| Region | Pool size | Per-frame cost |
|---|---|---|
| none | the window (even-cropped if asked) | none |
| at `(0, 0)` | the region | **none**: WGC clips into the pool |
| elsewhere | `(x + width, y + height)` | one `CopySubresourceRegion` into a ring slot |

- The pool never needs to be larger than the region's bottom-right corner, because WGC clips
  from the top-left. Rendering the whole window and copying out of it would make the
  compositor draw pixels nobody reads.
- With a region, the pool size depends on the region only, so **a window resize never
  recreates the pool.** What a resize can do is shrink the window below the region; that is
  `CaptureError::RegionOutOfBounds`, never padding. A padded frame contains pixels the source
  did not draw, which in a recording used as evidence can be mistaken for a rendering bug.
- `FrameDimensions::EvenCropped` applies to the region, because the region is what the encoder
  sees.
- The copied frame goes into a `CROP_RING_DEPTH = 4` ring, created on the first frame from
  WGC's own texture description (same format, bind flags, usage — which the encoder already
  accepts), and the WGC frame goes back to the pool immediately after the copy.

### Why a ring of four, and what it assumes

The WMF encoder wraps a delivered texture in an `IMFSample` instead of copying it, and an
async MFT may read it after `release_frame`. The uncropped WGC path already depends on that
read finishing within WGC's own two-buffer pool. The ring is twice that depth, so the crop
path is not the first thing to break if a pipeline turns out deeper. It is the same
assumption, stated. Guaranteeing it would need a GPU fence per slot.

### Other backends

| Backend | Region |
|---|---|
| Windows DXGI (screen) | refused. The shared path already copies each frame into its ring and could crop in that copy for free: follow-up. |
| macOS `ScreenCaptureKit` | refused. `SCStreamConfiguration::sourceRect` would crop natively: follow-up. |
| Linux portal, iOS | refused |

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Crop in a `FrameFilter` after capture | The filter layer rejects GPU frames. Making it handle them means threading a D3D11 context through a generic API for one operation the capture backend can do where the texture already lives. |
| Crop in the encoder (MF display aperture, SPS cropping) | Not reliably honoured by encoder MFTs, and it would leave the capture delivering pixels the caller explicitly did not want. |
| Always pool at the window size and copy | Pays a copy for every origin region. It also makes the compositor render pixels outside the region into the pool for nothing. |
| Clamp an out-of-bounds region to the surface | Frame size would change mid-stream, and a fixed-size encoder cannot follow. The caller asked for a rectangle that no longer exists; saying so is the honest answer. |
| Windows-only option (like `CaptureBorder`) | The region is content, not presentation, and `ScreenCaptureKit` has it natively. It belongs on the cross-platform config, with each backend honouring or refusing it, as `CursorCapture` does. |

## Consequences

### Positive

- A region at the origin costs nothing. Any other region costs one GPU copy of the region's
  own size, not the window's.
- Resizes no longer touch the pool when a region is set.
- Refusal is uniform: no backend silently records the whole surface.

### Negative / Trade-offs

- **Breaking:** another public field on `DesktopVideoCaptureConfig`; struct literals add
  `region: None`.
- The copied path is not Zero-Copy; it is `GpuCopy`, and documented as such on
  `CaptureRegion`.
- The ring depth is an assumption about encoder pipeline depth (above), not a guarantee.
- DXGI and `ScreenCaptureKit` refuse a region even though both could crop cheaply.

## Verification

- Pure tests of `CaptureRegion::fits_within` cover edges, an empty region, and `u32`
  overflow. `plan_crop` has tests for every branch: none, origin, offset (pool stops at the
  far corner), even-cropped region, out of bounds with its numbers, and a region that crops
  to nothing.
- DXGI refuses a region before touching a device.
- **Real window, pixel-exact** (live Unity editor, 2560×1392, Windows 11 Pro 26100,
  2026-09-19). Each region was captured and read back, then compared with the same rectangle
  of a full capture:

  | Region | Path | Differing pixels |
  |---|---|---|
  | 640×360 at (0, 0) | pool crop | 0 / 230 400 |
  | 640×360 at (137, 91) | GPU copy | 0 / 230 400 |
  | 500×300 at the far corner | GPU copy | 0 / 150 000 |

  A region past the edge was refused: *"capture region 64x64 at (2550, 0) does not fit a
  2560x1392 surface"*. To check that the comparison can fail, the copy box was shifted by
  one column. That gave 2 448 and 7 789 differing pixels on the two copy paths, and 0 on the
  pool path, which does not copy.

## References

- ADR-0008 — cursor, border, and the measurement that WGC crops rather than scales
- `crates/mediaway-device/src/windows_desktop/wgc.rs` — `plan_crop`, `deliver_region`
- [`ID3D11DeviceContext::CopySubresourceRegion`](https://learn.microsoft.com/en-us/windows/win32/api/d3d11/nf-d3d11-id3d11devicecontext-copysubresourceregion)
