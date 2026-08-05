# mediaway-encoder-vulkan — roadmap

Cross-platform Vulkan Video encode backend (`VK_KHR_video_encode_queue`).
Facade: [`mediaway-encoder`](../../mediaway-encoder/docs/roadmap.md).
Platform order: Windows → Web → Linux → other. Workspace index:
[`docs/roadmap.md`](../../../docs/roadmap.md). Sibling vendor/framework-axis
crates: `mediaway-encoder-nvenc`, `mediaway-encoder-quicksync`,
`mediaway-encoder-amf` (all ADR-only, not yet implemented).

## Stages

### 0 — Scaffold + capability probe (this change)

- [x] Workspace member + docs / ADR surface
- [x] `ash` dependency review + ADR ([0001](../adr/0001-vulkan-video-encode-ash-probe.md))
- [x] Real instance + physical-device + queue-family probe
      (`probe::probe_video_encode_queue_families`): reports which queue
      families advertise `VK_VIDEO_CODEC_OPERATION_ENCODE_H264_BIT_KHR` /
      `..._H265_BIT_KHR` per device
- [x] **Compiled / run** — hardware-verified same-day follow-up (ADR-0001
      § "Verification update"): found + fixed one real bug (`VK_KHR_video_queue`
      is a device extension, not an instance one), then confirmed against real
      hardware — RTX 4090 reports H.264/H.265 encode queue family `4`; Intel
      UHD 770's Windows Vulkan driver reports no encode queue at all (genuine
      finding, not a bug). `cargo test`/`clippy -D warnings`/`fmt --check` all
      clean.

### 1 — H.264 encode session — hardware-verified 2026-07-29 (see ADR-0001 § Stage 1 addendum)

- [x] `vkGetPhysicalDeviceVideoCapabilitiesKHR` (profile-chain query: picture
      layout, extent/alignment limits, DPB slot limits) — deeper than the
      Stage 0 queue-family flag check (`session.rs::query_capabilities`)
- [x] `VkVideoSessionCreateInfoKHR` + memory binding
      (`vkGetVideoSessionMemoryRequirementsKHR` /
      `vkBindVideoSessionMemoryKHR`) — `session_encode.rs::create_video_session`
- [x] `VkVideoSessionParametersCreateInfoKHR` +
      `VkVideoEncodeH264SessionParametersAddInfoKHR` (real SPS/PPS) —
      `h264_params.rs` + `session_encode.rs::create_session_parameters`
- [x] DPB image (`VkVideoPictureResourceInfoKHR` /
      `VkVideoReferenceSlotInfoKHR`) — single-slot/IDR-only, mirroring
      `mediaway-encoder-linux`'s "every frame independent IDR" stage
- [x] CPU-upload input path (`session_encode.rs::synthetic_gray_nv12` +
      `create_host_buffer`/`upload_to_host_memory` — a `copy`-class path, no
      Zero-Copy claim; real cost-disclosure naming deferred to a
      `mediaway_encoder::VideoEncoder` impl, not this diagnostic entry point)
- [x] `vkCmdBeginVideoCodingKHR` → `vkCmdEncodeVideoKHR` → `vkCmdEndVideoCodingKHR`
      command submission + fence wait — `session_command.rs`
- [x] Bitstream readback (mapped output buffer → Annex-B bytes) —
      `session_command.rs::submit_and_readback`; verified real (not just
      "has start codes") via independent system FFmpeg oracle: `ffprobe`
      parses `H.264 Baseline, 160x64, key_frame=1, level=10`; `ffmpeg` decodes
      with no errors to a pixel-exact match of the synthetic gray input
- [x] `impl mediaway_encoder::VideoEncoder for VulkanVideoEncoder` —
      **hardware-verified 2026-07-29** (see ADR-0001 § "VideoEncoder impl +
      HEVC addendum"): `encoder.rs`, a real, reusable, multi-frame session
      (instance/device/video session/session parameters/images/buffers/
      command pool/fence/query pool all persist across `push_frame` calls —
      only upload/record/submit/readback repeats per frame, mirroring
      `mediaway-encoder-windows`'s `D3d12VideoEncoder`). Real per-frame
      compressed-byte-count query (`VK_QUERY_TYPE_VIDEO_ENCODE_FEEDBACK_KHR`)
      closes [`encode_synthetic_intra_frame`]'s old "whole zero-padded
      buffer" scope cut — packets are now byte-exact.

Real session shape (types, ownership) mostly follows ADR-0001's original
design sketch, minus the `VideoSession<S>` typestate (deferred — see the
ADR's Stage 1 addendum "Deliberate scope cuts").

### 2 — GOP / rate control — H.264 + HEVC hardware-verified 2026-08-05 (see ADR-0002)

- [x] H.264: P-frame reference picture management, multi-slot DPB
      (`h264_gop.rs::GopState`/`Dpb`, `WORKSPACE_DPB_CAP = 4`) —
      hardware-verified real `I P P I P P I` cadence for `gop_size = 3`
- [x] H.264: CBR rate control (`session_command.rs`'s `DpbRecordParams`/
      `RateControlParams`, capability-gated via
      `Capabilities::supports_p_frames`/`supports_cbr`) — hardware-verified
      real CBR bitstream output
- [x] HEVC: same P-frame/DPB wiring — **hardware-verified 2026-08-05**
      (same-day follow-up): `hevc_gop.rs::GopState`/`Dpb` (reuses
      `h264_gop::WORKSPACE_DPB_CAP`), `hevc_params.rs`'s P-frame
      picture-info/slice-segment-header/reference-list/short-term-RPS
      builders, `session_command_hevc.rs`'s real setup/reference
      `VkVideoReferenceSlotInfoKHR` + `VkVideoEncodeH265DpbSlotInfoKHR`
      chaining — real `I P P I P P I` cadence for `gop_size = 3` (NAL type
      19 IDR / type 1 TRAIL_R P-slice). CBR rate control stays H.264-only
      this pass (ADR-0002's Decision section scopes it there); a
      `rate_control` request on an HEVC config is safely and silently
      ignored (falls back to fixed-QP, verified by
      `push_hevc_frames_gop_with_rate_control_requested_or_skip`).
- [x] AV1: same single-forward-reference P-frame/DPB wiring —
      **implemented, capability-gated, but genuinely unverifiable** on this
      crate's reference hardware (per an explicit later instruction to build
      it anyway, honestly labeled): `av1_gop.rs::GopState`/`Dpb`
      (`order_hint`-keyed, reuses `h264_gop::WORKSPACE_DPB_CAP`),
      `av1_params.rs`'s `INTER_FRAME` picture-info/reference-info builders,
      `session_command_av1.rs`'s real setup/reference
      `VkVideoReferenceSlotInfoKHR` + `VkVideoEncodeAV1DpbSlotInfoKHR`
      chaining, `Capabilities::supports_p_frames`'s AV1 floor check now real
      (`maxSingleReferenceCount >= 1`, was previously skipped). AV1's base
      (IDR-only) per-frame encode is already hardware-verified invalid on
      this hardware (ADR-0001's AV1 addendum), so this wiring inherits the
      same unverifiable status — `push_seven_av1_frames_gop_or_skip` hits
      that same known-broken bitstream and honestly skips, exactly like
      `push_three_av1_frames_or_skip` already does. No CBR for AV1 (same
      "H.264-only" reasoning HEVC's own bullet above already gives).

Design: [ADR-0002](../adr/vulkan/0002-vulkan-gop-rate-control.md) — P-frames
only (B-frames a permanent non-goal, they add reorder latency), CBR rate
control (H.264 only), capability-gated fallback to today's IDR-only/fixed-QP
behavior. AV1's GOP/DPB wiring is implemented (see above) but stays
unverifiable until its driver-blocked per-frame encode (ADR-0001 AV1
addendum) is confirmed fixed.

### 3 — Zero-Copy GPU input (deferred)

- [ ] Windows: `VK_KHR_external_memory_win32` import of a
      `GpuBufferHandle::DirectX11` (`ID3D11Texture2D`) as a Vulkan image
- [ ] Linux: `VK_KHR_external_memory_fd` (DMA-BUF) import
- [ ] `GpuBufferHandle::Vulkan` as Mediaway's own native path (no cross-API
      import needed when the caller is already Vulkan-native)

### 4 — Multi-codec

- [x] HEVC via `VK_KHR_video_encode_h265` — **hardware-verified 2026-07-29**:
      `hevc_params.rs` (`StdVideoH265*` VPS/SPS/PPS + picture-info/
      slice-segment-header builders), `session_command_hevc.rs` (per-frame
      recording — the one piece that couldn't share H.264's code, since the
      picture-info `pNext` payload type differs). Real finding:
      `picture_access_granularity` is **32x32** for HEVC on this driver, not
      16x16 like H.264 — the two must never be assumed equal.
- [x] AV1 via `VK_KHR_video_encode_av1` — implemented (`av1_params.rs`,
      `session_command_av1.rs`, `EncodeProfile::Av1`), migrated the crate from
      `ash` (no AV1 bindings) to `vulkanalia` to unlock it (see
      `adr/0001`'s migration addendum). **Blocked on this crate's reference
      RTX 4090, hardware-verified 2026-07-29** (see `adr/0001`'s AV1
      addendum): device/session/session-parameters/sequence-header all real
      and hardware-verified, but every `vkCmdEncodeVideoKHR` frame's own
      output is not a valid OBU stream — independently confirmed to be a
      driver-maturity limitation, not this crate's bug (`ffmpeg -c:v
      av1_vulkan` on the same machine produces AV1 output `dav1d` itself
      rejects). `push_three_av1_frames_or_skip` self-documents this and skips
      rather than hard-fails. Re-verify on a newer NVIDIA driver before
      assuming this is still broken.
