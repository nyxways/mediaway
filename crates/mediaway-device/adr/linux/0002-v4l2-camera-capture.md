# ADR-0002: V4L2 (`v4l` crate) for Linux camera capture

- **Status**: Accepted
- **Date**: 2026-07-29
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-device-linux`

## Context

`CaptureSource::Camera` had no Linux backend (ADR-0001 explicitly deferred
it). Video4Linux2 (`V4L2`) is the kernel-level camera capture API on Linux —
unlike screen capture, it needs no portal/compositor mediation: `/dev/video*`
character devices are opened directly, gated by Unix file permissions (the
`video` group), not a runtime consent dialog.

## Decision

> For [`CaptureSource::Camera`](../../mediaway-device/src/video.rs) with
> [`CaptureOutputPreference::CpuFramesOk`](../../mediaway-device/src/video.rs):
>
> 1. Enumerate `/dev/video*` via the `v4l` crate's `context::enum_devices()`,
>    numerically sorted by node index, filtered to nodes reporting
>    `V4L2_CAP_VIDEO_CAPTURE` (`VIDIOC_QUERYCAP`) — excludes the
>    metadata-capture-only sibling node many UVC webcams also expose. Ordinal
>    index `n` in this filtered list is `CaptureSource::Camera { device: n }`.
> 2. `VIDIOC_ENUM_FMT` to list formats the node actually advertises, then pick
>    the first available in priority order `YUYV` > `NV12` > `YU12` (I420) —
>    see § Format coverage.
> 3. Read the node's *current* format (`VIDIOC_G_FMT`) for a starting
>    width/height (falling back to 640×480 if unset), then `VIDIOC_S_FMT`
>    with that size + the chosen `FourCC`. Reject if the driver's "best
>    effort" response substitutes a different `FourCC` than requested — never
>    silently parse bytes as a layout we didn't actually get (same rule
>    `format::map_spa_video_format` documents for the screen-capture backend).
> 4. `mmap` streaming I/O (`VIDIOC_REQBUFS`/`QBUF`/`DQBUF`, 4 kernel-side
>    buffers) via `v4l::io::mmap::Stream`, polled with a 200 ms timeout on a
>    dedicated worker thread so `close()` can notice a stop flag without an
>    async cancel path (mirrors `mediaway-device-windows` `camera.rs`'s
>    "close can wait up to one frame interval" contract).
> 5. Each `mmap`-ed buffer's bytes are copied out row-by-row, honoring the
>    negotiated stride (driver alignment padding), into an owned
>    `VideoFrameStorage::Cpu` — same CPU-copy status as every other backend in
>    this crate this session.
>
> `CaptureOutputPreference::ZeroCopyGpu` → `CaptureError::Unsupported` (no
> `VIDIOC_EXPBUF`-based `DMABUF` export/GPU import this session — same
> deferral shape as ADR-0001's screen-capture DMA-BUF status).

### Format coverage: `YUYV` / `NV12` / `YU12`, not `MJPG`

Real UVC webcams commonly expose `MJPG` (compressed) and/or `YUYV` (raw
packed 4:2:2) natively; `NV12`/`YU12` (planar/semi-planar 4:2:0) are more
common on embedded/CSI cameras and some driver-side conversions. This
backend **does not** decode `MJPG` — that needs a JPEG decoder dependency,
out of scope here — so a camera whose driver offers none of the three raw
formats above has **no supported format this session**, and `open` returns
`CaptureError::Unsupported`. Unlike the Windows Media Foundation backend
(`mediaway-device-windows` `camera.rs`), there is no built-in
video-processor conversion step to fall back on.

`PixelFormat::Yuyv` (packed YUV 4:2:2) did not exist in `mediaway-common`
before this session — added as a new `#[non_exhaustive]` enum variant
(`formats.rs` already invites this: "Extend as backends need"). Purely
additive; no existing match arm breaks (every downstream crate already
carries a wildcard arm, required by `#[non_exhaustive]` outside the defining
crate).

## Why one function, not a split helper

`open_and_negotiate` returns an owned `v4l::Device` (not a borrow) so the
caller can build a `v4l::io::mmap::Stream` from it in the *same* stack frame.
`Stream<'a>`'s lifetime parameter ties to a `&'a Device` borrow in the
crate's public signature; rather than reason precisely about whether that
borrow is load-bearing for the `mmap` region's validity or just a
conservative API constraint, `run_camera_worker` keeps `Device` and `Stream`
alive in one function body for the whole capture loop — the same shape every
other backend in this crate already uses (COM/`WinRT` objects created and
torn down within one worker-thread closure in `mediaway-device-windows`).

## No `unsafe` in `camera.rs`

The `v4l` crate's whole surface used here (`Device`, `Capture`,
`io::mmap::Stream`, `CaptureStream`) is safe Rust — `mmap`/ioctl `unsafe`
lives entirely inside `v4l`/`v4l2-sys-mit`. `camera.rs` carries
`#![forbid(unsafe_code)]`, stricter than the crate root's
`#![allow(unsafe_code)]` for `target_os = "linux"` (same pattern
`format.rs`/`portal.rs` already use). This also means the stride-aware byte
extraction (`pack_frame_bytes`) works on a plain `&[u8]` and is directly unit
testable with synthetic buffers — no pointer/lifetime plumbing to fake, unlike
the Windows Media Foundation backend's `copy_2d_nv12`/`copy_2d_packed`
(needs a live `IMF2DBuffer`).

## Dependency review (`docs/conventions/deps-policy.md`)

### `v4l` 0.14 (`default-features = false, features = ["v4l2"]`)

| Check | Answer |
|-------|--------|
| Need | `std`/`libc`-only ioctl hand-rolling was seriously considered (see § Alternatives) but rejected: correctly encoding V4L2's `_IOWR`-style ioctl request numbers by hand is a real, silent-failure-prone surface (dir/type/nr/size bit packing), and `v4l` already solves it via `bindgen`-derived constants from the real system header. |
| License | MIT (both `v4l` itself and its default `v4l2-sys-mit` dependency — the "mit" in that package name signals it deliberately avoids the alternate `v4l-sys`/`libv4l` path, see next row). |
| Transitive (default path) | `bitflags` (MIT/Apache-2.0), `libc` (MIT/Apache-2.0), `v4l2-sys-mit` (MIT, `bindgen`-generated raw `<linux/videodev2.h>` bindings, **no runtime library link** — build-time header parsing only). No GPL/copyleft in the resolved (default-feature) graph. |
| Explicitly *not* taken | The crate's optional `libv4l` feature (→ `v4l-sys`, itself MIT-licensed but dynamically linking the LGPL-2.1 `libv4l1`/`libv4l2`/`libv4lconvert` userspace convenience libraries at runtime). Not needed: raw `videodev2.h` ioctls cover this backend's whole surface, and avoiding it means one fewer system runtime-library dependency (dynamic LGPL linking is an accepted pattern elsewhere in this crate — `pipewire`/`libpipewire-0.3` — but simpler to not need it at all here). |
| Maintenance | `raymanfx/libv4l-rs`, ~6.9M crates.io downloads, active (0.14.0, 2023 — V4L2's kernel UAPI is itself extremely stable, so a "last release 2023" library binding a stable ioctl surface is not a red flag the way it would be for a fast-moving API). |
| Build requirement | `bindgen` (already a workspace dep, used for `vpl-sys`) needs `libclang` at build time — confirmed present in this session's WSL2 environment (`libclang-18.so.18`, installed for the pre-existing `vpl-sys` build). A from-scratch Linux dev machine needs `libclang-dev` (or equivalent) in addition to `libpipewire-0.3-dev` (already required by ADR-0001) — a normal Linux dev-toolchain ask, not a runtime dependency. |
| Unsafe/FFI | `mmap`/ioctl `unsafe` lives inside `v4l`/`v4l2-sys-mit`; this crate's `camera.rs` call sites are all safe Rust (see § No `unsafe`). |

### Alternatives considered and rejected

| Alternative | Why not |
|-------------|---------|
| Raw ioctls via `libc` only, hand-derived `VIDIOC_*` request numbers | The Linux ioctl number encoding (`_IOC(dir, type, nr, size)` bit packing from `asm-generic/ioctl.h`) is easy to get subtly wrong by hand, and a wrong request number is a silent kernel-level footgun (`EINVAL`, or worse, memory corruption) rather than a compile error. `v4l2-sys-mit`'s `bindgen`-generated constants come from the same system header this backend would otherwise hand-transcribe — reinventing that derivation with zero ability to test it against real hardware this session (see § Zero runtime verification) is strictly higher-risk than depending on a widely used crate that already solved it. |
| GStreamer (`v4l2src` via `gstreamer-rs`) | Same rejection as ADR-0001's screen-capture ADR: pulls in the entire GStreamer runtime/plugin graph merely to reach V4L2 — far heavier than a direct binding, and GStreamer's own license mix needs separate review out of this ADR's scope. |
| `libv4l`-backed `v4l-sys` (the crate's `libv4l` feature) | Adds a runtime dependency on `libv4l2`/`libv4lconvert` (LGPL, dynamically linked) purely for userspace format-emulation convenience this backend doesn't need — `YUYV`/`NV12`/`YU12` are all raw kernel-reported formats already, no `libv4lconvert` emulation required. |

## Dependency caveat found this session (real, not hypothetical)

`v4l::io::mmap::Stream`'s `Drop` impl calls its internal `stop()`
(`VIDIOC_STREAMOFF`) unconditionally and **panics** if that ioctl fails for
any reason other than `ENODEV` (device unplugged, which it handles
gracefully). This is a real footgun in the dependency, not something this
crate's code controls — `Stream`'s `stop()` is not part of its public API to
call proactively and gate ourselves. In the ordinary "worker loop exits,
`Stream` drops" path this should not trigger (only unusual driver-level
`STREAMOFF` failures would), but it is worth a maintainer's attention if a
future report shows a panic originating inside `v4l`'s `Drop for Stream`.

## Zero runtime hardware verification this session

**No real V4L2 device was exercised in this development session.** WSL2
Ubuntu 24.04 has `/usr/include/linux/videodev2.h` (so `v4l2-sys-mit`'s
`bindgen` step can run) but **zero `/dev/video*` nodes** (confirmed via
`ls /dev/video*` — no such file or directory) — no virtual camera, no
passthrough USB webcam. Verification this session was **compile-only**
(`cargo check`/`clippy`/`test` for `mediaway-device-linux` under WSL2). The
hardware-gated tests (`camera_tests.rs`) are written to run the real path
and are expected to **skip** here for exactly this reason.

## Consequences

### Positive

- Real device enumeration, format negotiation, and `mmap` streaming I/O — not
  a stub — the only gap is hardware/session verification, matching this
  crate's existing screen-capture precedent (ADR-0001).
- Zero `unsafe` in this crate's own code for the camera path.
- No new runtime system-library dependency (unlike `pipewire`/ALSA) — only a
  build-time `bindgen` + header requirement.

### Negative / Trade-offs

- `MJPG`-only webcams (common for higher resolutions/framerates on cheap USB
  cameras) are unsupported until a JPEG decoder is added — real, current gap.
- `I420`'s chroma-plane stride is *assumed* (`stride / 2`, no independent
  per-plane stride in the single-planar V4L2 API) rather than read from the
  kernel — a documented approximation, not a verified-correct value for every
  driver.
- `v4l::io::mmap::Stream`'s panic-on-non-`ENODEV`-`STREAMOFF`-failure `Drop`
  behavior is inherited, unfixed dependency risk (see § Dependency caveat).

## References

- [`docs/conventions/deps-policy.md`](../../../docs/conventions/deps-policy.md)
- Screen-capture precedent: [ADR-0001](0001-portal-pipewire-screen-capture.md)
- Windows precedent: [`mediaway-device-windows/adr/... ` camera shape](../../mediaway-device-windows/src/camera.rs)
  (no dedicated ADR yet on that side — see that file's module docs)
- `v4l`: <https://github.com/raymanfx/libv4l-rs> (MIT)
- V4L2 API: <https://docs.kernel.org/userspace-api/media/v4l/v4l2.html>
