# mediaway-decoder-vulkan — roadmap

Cross-platform Vulkan Video decode backend (`VK_KHR_video_decode_queue`).
Facade: [`mediaway-decoder`](../../mediaway-decoder/docs/roadmap.md).
Platform order: Windows → Web → Linux → other. Workspace index:
[`docs/roadmap.md`](../../../docs/roadmap.md). Sibling portable
vendor/framework-axis crate: `mediaway-encoder-vulkan` (H.264/HEVC
hardware-verified encode, AV1 encode driver-blocked).

## Stages

### 0 — Scaffold + ADR + probe (2026-07-30)

- [x] Workspace member + `docs/` / `adr/` surface
- [x] `vulkanalia` dependency reuse (no new Cargo dependency) + ADR
      ([0001](../adr/0001-vulkan-video-decode.md)) — decode struct-binding
      survey via `docs.rs` (H.264/HEVC/AV1 picture-info + DPB-slot structs
      all confirmed present), general-GOP + three-codec scope decision
- [x] Real instance + physical-device + decode queue-family probe
      (`probe::probe_video_decode_queue_families`), hardware-verified
      2026-07-30: **both** an RTX 4090 (queue family
      3) and Intel UHD 770 (queue family 1) advertise H.264/H.265/AV1 decode
      queue families — a real, positive result, better than the encode
      side's Intel finding. See ADR-0001's 2026-07-30 addendum.

### 1 — H.264 decode, general GOP (2026-07-30: complete + hardware-verified)

- [x] `VkVideoSessionKHR` decode session + capability query (`session.rs`)
- [x] SPS/PPS parse (`h264_params.rs`, reusing `mediaway_sw::h264::BitReader`
      + `NalUnit`/`split_annex_b` for framing — **not**
      `mediaway_sw::h264::{Sps, Pps}`, which are IDR-only-shaped)
- [x] Slice-header parse (I/P; B rejected this round) + `RefPicList0`
      construction + application (`h264_slice.rs`) — sans-io, not fed to any
      Vulkan call this round (see that module's doc: the hardware parses
      `ref_pic_list_modification()`/`dec_ref_pic_marking()` itself from the
      raw bitstream bytes; no `StdVideoDecodeH264ReferenceListsInfo` struct
      exists in `vulkanalia` 0.35 to feed it through anyway)
- [x] DPB with sliding-window reference management + `FrameNumWrap`
      recomputation + IDR clear-all + Zero-Copy backpressure (`dpb.rs`) —
      sans-io, unit-tested without a Vulkan device (18 tests)
- [x] `vkCmdDecodeVideoKHR` submission (`session_command.rs` +
      `session_command_h264.rs`) — **hardware-verified**: real decode target
      layout transitions (`VIDEO_DECODE_DPB_KHR` ⇄ `VIDEO_DECODE_DST_KHR`),
      correct `slotIndex = -1` reference-slot activation protocol, and a
      real Annex-B start code in the uploaded bitstream (three real bugs
      found by comparing field-by-field against FFmpeg's own
      `vulkan_decode.c`/`vulkan_h264.c` — see ADR-0001's second 2026-07-30
      addendum)
- [x] CPU readback (`cpu_readback.rs`) — verified correct both standalone
      (upload-then-readback round-trip) and as part of the full decode path
- [x] Zero-Copy GPU output via `GpuBufferHandle::Vulkan` (`zero_copy.rs`)
- [x] `impl mediaway_decoder::VideoDecoder for VulkanVideoDecoder`
      (`decoder.rs`)
- [x] **Hardware-gated integration test passes with hard assertions**
      (`tests/hardware_h264_decode.rs`, `cargo test -p mediaway-decoder-vulkan
      --test hardware_h264_decode`): a hand-crafted IDR + P-frame stream
      decodes to the exact expected literal pixel values, including a real
      motion-compensated `P_Skip` DPB reference read and genuinely new
      `I_PCM` P-frame content — real H.264 general-GOP decode, verified on
      the RTX 4090.

### 2 — HEVC (2026-07-30: sans-io complete + tested; GPU decode not yet hardware-verified)

- [x] Own VPS/SPS/PPS parser (`hevc_params.rs`) — 2-byte NAL header, new
      code (not shared with H.264's 1-byte header parse); `StdVideoH265*`
      struct bindings confirmed directly against `vulkanalia-sys` 0.35's
      vendored source (not inferred)
- [x] Slice-segment-header parse + short-term RPS construction
      (`hevc_slice.rs`) — genuinely new logic (POC-based RPS, not H.264's
      `frame_num` sliding window), sans-io, unit-tested without a Vulkan
      device (19 new tests, 62 total for this crate's `--lib` suite)
- [x] Per-frame `vkCmdDecodeVideoKHR` recording (`session_command_hevc.rs`,
      `decoder_hevc.rs`) — mirrors the verified H.264 command sequence,
      **IDR pictures only this round** (a P/B-slice HEVC NAL returns
      `DecodeError::Unsupported` — general-GOP HEVC decode is follow-up work)
- [ ] **Hardware verification — not yet achieved.**
      `tests/hardware_hevc_decode.rs` chains this workspace's own
      hardware-verified `mediaway-encoder-vulkan::VulkanVideoEncoder` (real
      bitstream, not hand-crafted CABAC) into `VulkanVideoDecoder`; two real
      bugs were found and fixed (`HevcSps`/`HevcPps::to_std` silently
      zeroing several `Std*Flags` bits regardless of what the real encoder
      signaled), but the decoded picture still reads back all-zero — root
      cause not yet found. Test soft-skips loudly rather than hard-failing
      the default suite. See ADR-0001's 2026-07-30 HEVC addendum for the
      full account, ruled-out hypotheses, and open leads.

### 3 — AV1

- [ ] OBU scan + sequence-header/frame-header parse (`av1_params.rs`) — new,
      no existing parser in this workspace (`mediaway_sw::av1` is a `rav1e`
      **encoder** adapter, not a parser)
- [ ] `ref_frame_idx` reference management (`av1_refs.rs`)
- [ ] Per-frame `vkCmdDecodeVideoKHR` recording (`session_command_av1.rs`)
- [ ] **Film-grain synthesis deferred** — base decode first, grain synthesis
      as a separate, explicitly-verified follow-up (ADR-0001 § AV1 film
      grain)

### 4 — Deferred / explicit scope cuts (see ADR-0001)

- [ ] 10/12-bit profiles (blocked on a `PixelFormat` gap in
      `mediaway-common` — no 10-bit variant exists today)
- [ ] HEVC scalability/RExt/SCC, tiles/WPP beyond single-tile
- [ ] Interlaced/field pictures
- [ ] Long-term reference marking beyond a sliding window
- [ ] Windows `VK_KHR_external_memory_win32` / Linux `VK_KHR_external_memory_fd`
      cross-API Zero-Copy interop (native `GpuBufferHandle::Vulkan` output
      is in scope; importing a *foreign* API's handle as a Vulkan image is not)
