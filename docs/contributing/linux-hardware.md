# Verifying the Linux backends on real hardware

The Linux backends were written and compiled on Windows with WSL2, which has no usable `/dev/dri` device,
so their tests skip. This page is for the first session on a Linux machine that has the hardware. It closes
two open items in [`docs/roadmap.md`](../roadmap.md): **Linux Hardware Verification** (VA-API encode and
decode) and **Linux Capture Hardware Verification** (portal screencast, PipeWire microphone, V4L2 camera).

A test that cannot open a device prints `skip: ...` and **passes**. A green run on a machine without the
device therefore verifies nothing, and the report script below counts the skips for that reason.

## What is under test

| Area | Where | Needs |
|---|---|---|
| VA-API decode (H.264, HEVC, VP9, AV1 key frames) | `crates/mediaway-decoder/src/linux/` | a GPU with a VA-API driver and `/dev/dri/renderD*` |
| VA-API encode (H.264, HEVC, VP9) | `crates/mediaway-encoder/src/linux/` | the same, with encode entry points (`vainfo` lists `VAEntrypointEnc*`) |
| Vulkan Video (HEVC decode, AV1) | `crates/mediaway-decoder`/`-encoder` `src/vulkan/` | a Vulkan driver with `VK_KHR_video_*` (Mesa RADV or ANV recent enough, or NVIDIA) |
| Screen capture | `crates/mediaway-device/src/linux/{portal,screencast}.rs` | a graphical session and `xdg-desktop-portal` with a backend for the desktop |
| Microphone | `crates/mediaway-device/src/linux/mic.rs` | PipeWire running with an input |
| Camera | `crates/mediaway-device/src/linux/camera.rs` | a V4L2 capture device (`/dev/video*`) |

The decisions behind each are in the `adr/linux/` folders of the three crates and in
`crates/mediaway-device/adr/0003-capability-and-permission-probe.md`.

## Prerequisites

Packages (Debian/Ubuntu names; other distributions have the same libraries under their own names):

```bash
sudo apt-get install -y build-essential pkg-config libclang-dev \
    libva-dev libpipewire-0.3-dev libspa-0.2-dev \
    vainfo vulkan-tools v4l-utils pipewire-bin wireplumber
```

Rust is the version in [`rust-toolchain.toml`](../../rust-toolchain.toml) and `cargo install cargo-nextest --locked`.
Use a **real session on the machine** (a desktop login, not an SSH shell): screen capture asks the portal, and
the portal needs the user's session bus. The user must be in the `render` and `video` groups for `/dev/dri` and
`/dev/video*`.

## Run

```bash
tools/scripts/linux-hardware-report.sh
```

It writes `target/linux-report/<host>-<date>.md` with the machine (GPU, `vainfo`, `vulkaninfo`, cameras,
PipeWire, the portal) and then runs `cargo nextest run` for the decoder, encoder and device crates with
`--no-capture`, one test at a time, and lists every `skip:` line it saw. Pass other packages with
`MEDIAWAY_LINUX_PACKAGES="-p mediaway-device"`.

**Read the skips first.** For the backend being verified the count must be zero:

- `skip: no VA-API display available` — no render node, or no VA-API driver. Check `vainfo` in the report.
- A capture test that cannot reach the portal or PipeWire prints its own `skip:` with the error.

## First things to check, in order

1. `vainfo` lists the codec's profile **and** the entry point (`VAEntrypointVLD` for decode,
   `VAEntrypointEncSlice`/`EncSliceLP` for encode). If it does not, the driver lacks it: not a mediaway bug.
2. VA-API decode of H.264 to CPU frames, then HEVC, VP9, AV1 (the order of the ADRs). Compare pixels, not
   "no error": a decode that returns without an error but with wrong pixels is the failure the Vulkan HEVC
   item in the roadmap had.
3. VA-API encode, then decode the output with `ffprobe`/`ffmpeg` (the oracle tests, `ffmpeg` on `PATH`) to prove
   the bitstream is valid. A stream that the encoder accepts and no decoder does is the Vulkan AV1 failure
   mode already in the roadmap.
4. Screen capture: the first run opens the portal's picker, which a person must answer. Capture one frame
   and check it is not black (a black frame is the usual failure of a wrong DMA-BUF modifier).
5. Microphone and camera: enumerate, open, take a second of data, check format and timestamps.
6. Hotplug: unplug the camera while capturing; the session must end with an error, not hang.

## What to do with a result

- A backend that passes: change its roadmap item to `[x]` with the machine (GPU, driver and Mesa version,
  kernel, distribution) and the report file's name, in the same commit as any fix. Say **which** codecs
  passed; "VA-API works" is not a claim the tests support for a codec they do not cover.
- A failure that is a driver gap: record it in the backend's ADR as an addendum, as the Vulkan AV1 item does.
- A failure that is a mediaway bug: fix it, and keep the test (without a `skip:` path that hides it).
- Keep the report out of the repository (`target/` is ignored); paste its summary into the roadmap entry.

## What this page does not cover

- **Continuous integration on a GPU.** GitHub-hosted runners have no GPU, so CI keeps compiling, linting and
  running the tests that skip; whether a self-hosted GPU runner is worth having is a decision for the owner.
- **Software Vulkan (`lavapipe`)** gives a Vulkan device without a GPU, but it has no `VK_KHR_video_*`
  extensions that this project's tests need; it is not a substitute (not verified on a recent Mesa).
- Linux packaging of the bindings (`.so`, wheels, npm) is done and covered by ADR-0024; this page is about
  hardware, not distribution.
