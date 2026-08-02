# mediaway-encoder-quicksync — roadmap

Intel Quick Sync / Arc (oneVPL) direct-vendor video encode backend (`Backend::QuickSync`).
Facade: [`mediaway-encoder`](../../mediaway-encoder/docs/roadmap.md).
Platform order: Windows → Web → Linux → other. Workspace index:
[`docs/roadmap.md`](../../../docs/roadmap.md).

## Stages

### 0 — Scaffold + design

- [x] Workspace member + docs / ADR surface
- [x] `vpl-sys` binding design + ADR ([0001](../adr/0001-onevpl-quicksync-encode-surface.md))

### 1 — oneVPL H.264 CPU-upload (this change)

- [x] `vpl-sys`: vendored `mfxdefs.h`/`mfxcommon.h`/`mfxstructures.h` (pinned commit), `bindgen`
      types-only build (`ignore_functions()`), hand-transcribed constants + `extern "system"`
      function-pointer signatures
- [x] `vpl-sys::dispatcher`: MVP `libloading` dispatcher (`libmfxhw64.dll` on Windows), ~10
      resolved entry points (`MFXInitEx`/`MFXClose`/`MFXQueryVersion`/`MFXQueryIMPL`/
      `MFXVideoENCODE_{Query,Init,Close,EncodeFrameAsync}`/`MFXVideoCORE_{SyncOperation,SetHandle}`)
- [x] Real session open against `MFX_IMPL_HARDWARE_ANY` (see that constant's rustdoc — hardware
      finding, not upstream-documented) — **hardware-verified** on an Intel UHD 770
- [x] H.264 Baseline, real I/P GOP (`GopPicSize`/`GopRefDist`, driver-managed references — not
      all-IDR, unlike the Linux VA-API stage), CQP or CBR (`bitrate_bps == 0` selects CQP)
- [x] CPU NV12 upload (`upload_cpu_nv12`, documented cost — 16-aligned internal buffer, real
      per-frame copy)
- [x] `MFXVideoENCODE_EncodeFrameAsync` + `MFXVideoCORE_SyncOperation` push/sync/collect loop,
      `MFX_WRN_DEVICE_BUSY` retry, `MFX_ERR_MORE_DATA`-driven flush drain
- [x] **Hardware-verified real encode**: `cargo test -p mediaway-encoder-quicksync -- --nocapture`
      produces real Annex-B H.264 (SPS/PPS/IDR + P-slice NALs) from the real Intel UHD 770 —
      see `adr/0001`'s 2026-07-29 addendum for full output
- [x] `cargo deny check` clean

**Wired into `mediaway-encoder-windows`'s `AutoVideoEncoder`** via `BackendSelection`
(`mediaway-encoder::auto`, ADR-0004) — `AutoVideoEncoder::open` opens QuickSync directly
for `Explicit(Backend::QuickSync)`, and tries NVENC then QuickSync ahead of `Os` CPU
upload for `AutoHardwareOnly`; never reached by plain `Auto`, per ADR-0004's "vendor SDK
not default #1".

### 2 — Backend-preference integration

- [x] Wire into `mediaway-encoder`'s `auto`/`EncodePathClass`/backend-preference (ADR-0004)
- [ ] `MFXVideoENCODE_GetVideoParam` / extradata (SPS/PPS) exposure via `StreamInfo::extra_data`
      if a caller needs it outside the embedded-in-bitstream form already produced

### 3 — Zero-Copy D3D11 (deferred)

- [ ] External frame allocator (`mfxFrameAllocator`, `GetHDL` -> `mfxHDLPair`) for
      `VideoInputPreference::ZeroCopyGpu` over `GpuBufferHandle::DirectX11`
- [ ] `MFXVideoCORE_SetHandle(MFX_HANDLE_D3D11_DEVICE, …)` wiring (already declared/resolved in
      `vpl-sys::dispatcher`, unused this stage)

### 4 — Multi-codec / Linux (HEVC + AV1 attempt done; Linux still deferred)

- [x] HEVC — `vpl-sys::consts` gained `MFX_CODEC_HEVC`/`MFX_PROFILE_HEVC_MAIN`/
      `MFX_LEVEL_HEVC_41` (hand-transcribed, cited by header line). `QuickSyncSession::open`
      selects `CodecId`/`CodecProfile`/`CodecLevel` via a new `codec_params(CodecKind) ->
      Result<(u32, u16, u16), EncodeError>` helper — same `MFXVideoENCODE_*` entry points, same
      push/flush/GOP path as H.264, no codec-specific plumbing needed beyond that helper.
      **Hardware-verified real encode**: `real_hevc_encode_produces_vps_sps_pps_idr_or_skips`
      produces genuine VPS(32)/SPS(33)/PPS(34) + IDR(19) then P-slice(1)/SEI(39) HEVC NALs on
      the Intel UHD 770 — see `adr/0001`'s 2026-07-29 HEVC/AV1 addendum.
- [x] AV1 — `vpl-sys::consts` gained `MFX_CODEC_AV1`/`MFX_PROFILE_AV1_MAIN`/`MFX_LEVEL_AV1_41`.
      `codec_params`/`validate` accept `CodecKind::Av1` and attempt the identical
      `MFXVideoENCODE_Query`/`_Init` path honestly (no special-casing to refuse it upfront).
      **Real result on this Intel UHD 770 (Alder Lake / Xe-LP)**: `MFXVideoENCODE_Query` returns
      `MFX_ERR_UNSUPPORTED` (`mfxStatus -3`) — this generation's iGPU genuinely does not support
      AV1 hardware *encode* (AV1 *decode* is a separate, unrelated capability), confirmed by a
      dedicated diagnostic test (`av1_encode_query_reports_real_hardware_result_or_skips`, talks
      to `vpl-sys` directly so the exact `mfxStatus` is captured — `QuickSyncSession`'s public
      `EncodeError` intentionally does not carry it). See `adr/0001`'s addendum for full output.
      Not forced further (no `MFXVideoENCODE_Init` attempt possible once `Query` itself rejects
      the codec).
- [ ] VP9 (still not supported by `vpl-sys::consts` — no constants added this stage)
- [ ] Linux (`libmfxhw64.so`-equivalent search candidates in `vpl-sys::dispatcher`) — **zero
      Linux verification** this stage (no Linux Intel GPU available in this session); Windows-only
      `cfg` gate stays until that changes
- [ ] Real official-dispatcher (`MFXLoad`/`MFXCreateConfig`/`MFXEnumImplementations`) support as
      an alternative to the MVP `libloading` dispatcher, if multi-adapter/capability-filtering
      needs outgrow the MVP's "first working Intel GPU implementation wins" policy
