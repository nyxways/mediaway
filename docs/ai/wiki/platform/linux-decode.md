# Linux decode (VA-API)

- Module: `mediaway-decoder::linux`
- Bindings: [`cros-libva`](https://crates.io/crates/cros-libva) (BSD-3-Clause, `cfg(target_os =
  "linux")` dependency only — never pulled into non-Linux builds)
- Codec: H.264 baseline/main (`vaapi/codec.rs` → `VAProfileH264ConstrainedBaseline` /
  `VAProfileH264Main`) and HEVC Main (`VAProfileHEVCMain`, ADR-0003), both `VAEntrypointVLD`.
  Dispatched behind a `VaapiVideoSession` enum (`H264(VaapiH264Decoder) | Hevc(VaapiHevcDecoder)`,
  no `Box<dyn>`) in `vaapi/mod.rs`; `LinuxVideoDecoder::open` picks a variant from `config.codec`.
- Scope: I and single-forward-reference P slices (real GOP/IPPP... decode), single slice per
  picture, `pic_order_cnt_type == 0`, progressive. Sliding-window DPB (`vaapi/dpb.rs`, ported
  from `vulkan/dpb.rs`), `RefPicList0[0]`-only reference (no reordering, no long-term refs, no
  weighted prediction, no CABAC P-slices, no B-slices, no multi-reference — all rejected
  honestly). Own H.264 SPS/PPS/slice-header parser (`vaapi/sps.rs`/`pps.rs`/`slice.rs`) reuses
  [`mediaway_sw::h264`](../../../crates/mediaway-sw/docs/roadmap.md)'s `BitReader` + Annex-B
  NAL splitting — VA-API (unlike Windows Media Foundation) requires the *application* to
  parse SPS/PPS/slice headers itself.
- CPU: NV12 readback via `Picture::create_image` (`vaCreateImage`+`vaGetImage`) — pitches/
  offsets stripped by `vaapi/nv12.rs` into a tightly packed buffer (same layout as
  `mediaway-decoder::windows`'s `wmf/cpu.rs`)
- Zero-Copy: **not implemented** — `VideoOutputPreference::ZeroCopyGpu` returns `Unsupported`
  (deferred: DMA-BUF surface export, `vaExportSurfaceHandle`)
- Session shape: `Display`/`Config`/`Context`/`Surface` (safe `cros-libva` wrappers) +
  `Picture<S, T>` **typestate** enforcing `vaBeginPicture → vaRenderPicture → vaEndPicture →
  vaSyncSurface` ordering at compile time; pipeline creation is **lazy** (first SPS seen),
  since `open()` cannot know profile/coded resolution before that
- ADR: [0001](../../../../crates/mediaway-decoder/adr/linux/0001-vaapi-h264-cpu-out.md) —
  binding choice, decode scope, **zero real-hardware verification** caveat
- ADR: [0002](../../../../crates/mediaway-decoder/adr/linux/0002-vaapi-h264-p-slice-dpb.md) —
  **Implemented, WSL2 compile+test-verified**: extends decode to
  single-forward-reference P-slices (+ free non-IDR I-slices) by porting the
  sliding-window DPB / POC arithmetic from `vulkan/dpb.rs` /
  `vulkan/h264_slice.rs` / `vulkan/h264_params.rs` into a new, sans-io
  `vaapi/dpb.rs`; no B-slices, no reference-list reordering, no weighted
  prediction, no CABAC P-slices, no multi-reference this round. Surface pool
  is DPB-slot-indexed (round-robin removed), sized `sps.max_num_ref_frames +
  1`. Found two real latent gaps in the Vulkan porting source itself
  (`pred_weight_table()`, `cabac_init_idc` unhandled) — rejected honestly
  here rather than inherited silently. Still zero real VA-API hardware
  verification.

- ADR: [0003](../../../../crates/mediaway-decoder/adr/linux/0003-vaapi-hevc-p-slice-dpb.md) —
  **Implemented, WSL2 compile+test-verified**: HEVC Main profile single-forward-reference
  P-slice decode. Unlike ADR-0002's H.264 port, **no hardware-verified porting source existed**
  — Vulkan's own HEVC decode is IDR-only in practice — so this is a **fresh design**: a small
  single-slot `HevcDpb` (`vaapi/hevc_dpb.rs`, reuses `vaapi/dpb.rs::derive_pic_order_cnt_msb`
  directly rather than re-deriving it — ITU-T H.265 §8.3.1 shares H.264 §8.2.1.1's MSB/LSB
  formula), and a slice-header parser (`vaapi/hevc_slice.rs`) extended well past
  `vulkan::hevc_slice.rs`'s own stopping point (VA-API needs many explicit per-slice fields —
  SAO, temporal-MVP, merge-cand count, QP deltas — that Vulkan Video's hardware parses from raw
  bytes directly). `vaapi/hevc_sps.rs`/`hevc_pps.rs` start from `vulkan::hevc_params.rs` and add
  a real new PPS-parsing tail VA-API's own parameter buffers require
  (`log2_parallel_merge_level_minus2`, `lists_modification_present_flag`, plus three new
  reject-if-set extension flags). VA-API decode's `PictureParameterBufferHEVC` needs **no
  VPS-derived field at all** (confirmed from `cros-libva`'s real source) — no `hevc_vps.rs`,
  unlike the Vulkan decode module's `HevcVps` requirement. RPS shape is validated to exactly
  `num_negative_pics == 1, delta_poc == -1, used_by_curr_pic` — anything else rejected as
  `Unsupported`; CRA/random-access pictures are a permanent scope cut this session.

## ⚠️ Hardware verification status

Written and compile-verified on Linux (WSL2 Ubuntu 24.04, real `libva-dev` headers/bindgen
output) but **never run against a real VA-API display/driver**. The dev box is Windows (no
native VA-API); the available WSL2 instance has broken VA-API (`vainfo` segfaults) and only
software `llvmpipe` Vulkan. `Display::open()` honestly returns `DecodeError::Unsupported` in
that environment — this is the expected, documented outcome, not a bug. Treat every VA-API
call path, and every H.264 bitstream-parsing decision (especially the
`slice_data_bit_offset` arithmetic), as unverified against a real encoder's bitstream until
run on real Linux + VA-API hardware (Intel iHD / Mesa / AMD).

## Structural differences vs. Windows (WMF) CPU decode

| Windows | Linux | Note |
|---------|-------|------|
| Software H.264 decoder MFT parses the whole bitstream internally | Driver does entropy decode + reconstruction; **this crate** parses SPS/PPS/slice headers | VA-API's app/driver split, not MF's black-box MFT |
| `IMFTransform` (one object) | `Config` + `Context` (capability vs. bound session) | VA-API separates driver capability from session |
| Async event pump for HW MFTs | None needed | `vaSyncSurface` blocks; fully synchronous per picture |
| DX11 Zero-Copy decode out | Not implemented this stage | See ADR-0001 § Scope |
