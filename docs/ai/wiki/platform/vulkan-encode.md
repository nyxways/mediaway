# Vulkan Video encode (`mediaway-encoder::vulkan`)

Module: `mediaway-encoder::vulkan` (portable — not OS-suffixed; Vulkan Video is
one API across Windows/Linux/Android). ADR:
[`0001-vulkan-video-encode-ash-probe.md`](../../../../crates/mediaway-encoder/adr/vulkan/0001-vulkan-video-encode-ash-probe.md).

**Binding crate: `vulkanalia`, not `ash`** (migrated 2026-07-29, ADR-0001's
migration addendum) — `ash` 0.38 had no `VK_KHR_video_encode_av1` bindings and
no committed timeline for them; `vulkanalia` has complete AV1 bindings and is
actively released. Zero H.264/HEVC regression from the swap (byte-identical
hardware re-verification).

## Stage 0: capability probe (real, hardware-verified)

`probe::probe_video_encode_queue_families` — instance + physical
device + `vkGetPhysicalDeviceQueueFamilyProperties2` chained with
`VkQueueFamilyVideoPropertiesKHR`. On the test machine:
RTX 4090 advertises H.264 **and** H.265 encode on queue family 4; Intel UHD
770's Windows Vulkan driver advertises **no** video-encode queue at all — a
real per-driver finding, not a probe bug. Real bug caught by the hardware
run: `VK_KHR_video_queue` is a **device** extension, not an instance one —
every driver rejected the first draft's `InstanceCreateInfo` enablement with
`VK_ERROR_EXTENSION_NOT_PRESENT`.

## Stage 1: minimal H.264 encode (real, hardware-verified, 2026-07-29)

`session_encode::encode_synthetic_intra_frame` — one real, hardware-run
all-intra H.264 frame end to end. Not a `mediaway_encoder::VideoEncoder`
impl, not multi-frame, not rate-controlled, not Zero-Copy.

```mermaid
flowchart TD
    A["VkVideoSessionKHR create"] --> B["vkGetVideoSessionMemoryRequirementsKHR"]
    B --> C{"memory type bits"}
    C -->|"expected DEVICE_LOCAL"| X["fails: matches only a\nHOST_VISIBLE|HOST_COHERENT|HOST_CACHED heap"]
    C -->|"no property flags required\n(opaque driver state)"| D["bind session memory ok"]
    D --> E["VkVideoSessionParametersKHR\n(real SPS/PPS via h264_params)"]
    E --> F["DPB + input images\n(DEVICE_LOCAL required here)"]
    F --> G["vkCmdBeginVideoCodingKHR"]
    G --> H["vkCmdEncodeVideoKHR"]
    H --> I["vkCmdEndVideoCodingKHR"]
    I --> J["bitstream buffer readback"]
    J --> K["nal.rs scanner: SPS(7)/PPS(8)/IDR(5)"]
```

Result on the RTX 4090: 160x64 frame, 4115 bitstream bytes, Annex-B
SPS(7)/PPS(8)/IDR(5) — confirmed by this crate's own scanner **and**
independently by a system-FFmpeg oracle (`ffprobe` parses the SPS correctly;
`ffmpeg` decodes to a pixel-exact match of the synthetic gray input, every
luma byte `0x80`). Real blocker hit and fixed: the memory-type-bits branch
above — `vkGetVideoSessionMemoryRequirementsKHR` returned a bind requirement
matching only memory type 3 (confirmed via `vulkaninfo`'s heap dump to be
`HOST_VISIBLE|HOST_COHERENT|HOST_CACHED`, non-device-local) — session memory
itself needs no specific property flags, only the DPB/input images do.

## HEVC + AV1 (2026-07-29)

HEVC: `hevc_params.rs`/`session_command_hevc.rs`, hardware-verified real
VPS/SPS/PPS + slice-segment encode. `picture_access_granularity` is **32x32**
on this driver, not 16x16 like H.264 — query per codec, never assume equal.

AV1: `av1_params.rs`/`session_command_av1.rs`, implemented but **blocked on
the RTX 4090** — device/session/session-parameters/sequence-header
are all real and hardware-verified (`vkGetEncodedVideoSessionParametersKHR`
returns a genuine `OBU_SEQUENCE_HEADER`), but every `vkCmdEncodeVideoKHR`
frame's own output is not a valid OBU stream. Independently confirmed **not**
this crate's bug: `ffmpeg -c:v av1_vulkan` on the same test machine produces AV1
`dav1d` itself rejects (decode error rate up to 73%) — a driver-maturity
limitation. See ADR-0001's AV1 addendum for the full field-by-field
debugging trail (6 real bugs found + fixed by diffing against FFmpeg's
`vulkan_encode_av1.c`) before this conclusion. `push_three_av1_frames_or_skip`
self-documents and skips rather than hard-fails.

## Scope cuts (all documented in code + ADR)

- No `VK_LAYER_KHRONOS_validation` (not installed, no Vulkan SDK on the test
  machine) — validated instead against `vulkanalia`'s generated `Extends*KHR`
  marker traits (compiler-checked, generated from the Vulkan registry).
- No exact-byte-count query-pool feedback for Stage 1's one-shot diagnostic
  (fixed for the real `VideoEncoder` impl via `VK_QUERY_TYPE_VIDEO_ENCODE_FEEDBACK_KHR`).
- No per-object RAII on error paths in the Stage 1 diagnostic — single-shot,
  not a reusable session (the real `VideoEncoder` impl tears down explicitly).
- Coarse (not perf-tuned) `sync2` barriers.

## Related

- [linux-encode](linux-encode.md), [windows-encode](windows-encode.md) — OS-suffixed sibling backends
- README § GPU — by API, Vulkan column
