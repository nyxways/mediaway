# mediaway-encoder — roadmap

**Facade** crate (traits). Platform backends: `mediaway-encoder-windows`, `mediaway-encoder-web`, …  
Packaging: [`docs/spec/crate-packaging.md`](../../../docs/spec/crate-packaging.md).  
Platform order: **Windows → Web → Linux → other**.  
Workspace index: [`docs/roadmap.md`](../../../docs/roadmap.md).

## Stages

### 0 — Scaffold

- [x] Facade crate + `docs/` / `adr/`
- [x] ADR: `VideoEncoder` / `AudioEncoder` traits + streaming poll API
- [x] ADR: facade vs `mediaway-encoder-<platform>` boundary

### 1 — Windows

- [x] Add `mediaway-encoder-windows` workspace member
- [x] WMF H.264 encode (sync inbox MFT, CPU NV12 upload)
- [x] DX11 texture Zero-Copy push path (HW MFT + DXGI)
- [x] `auto` types in facade (ADR-0003); `AutoVideoEncodeConfig::new` (explicit size)
- [x] Windows `AutoVideoEncoder::open` / `WindowsVideoEncoder::open` (no free `open_*`)
- [x] GpuCopy path in `auto` (`DirectX12` → `D3d12SharedEncodeBridge`)
- [ ] Readback / SW paths in `auto` (policy bits recognized; no backend yet — honest `NoBackend` error)
- [x] WMF AAC encode
- [x] Integration smoke with `mediaway-container` + `mediaway-test-media`
- [x] WMF HEVC/AV1/VP9 encode dispatch (CPU `MFTEnumEx` + DX11 Zero-Copy, no hardcoded
      CLSID) — ADR-0004; a later premise that AV1 dispatch still needed "wiring up" was
      wrong, see ADR-0010
- [x] WMF AV1 `av1C` config-record correctness — `refresh_extradata` is now codec-aware:
      `iso_bmff::bitstream::av1::to_av1c` (new, sans-io, unit-tested incl. a real
      `ffmpeg`/`libaom-av1` oracle test) builds a real `AV1CodecConfigurationRecord` from the
      Sequence Header OBU; H.264 keeps `avc::to_avcc`, HEVC/VP9 keep the pre-existing
      raw-bytes-verbatim fallback (known separate gap, not fixed here) — ADR-0010 implemented.
      Profile/level/tier bitfields stay zero (deferred per ADR-0010, no confirmed real MFT
      output to verify field population against yet).
- [x] Real `MFTEnumEx(MFT_CATEGORY_VIDEO_ENCODER, …)` encoder probe
      (`wmf::video::tests::list_encoder_mfts_for_each_codec`) — ADR-0010 implemented. **Real
      finding on this session's verification host (RTX 4090 + Intel UHD 770, 2026-08-19)**:
      an AV1 encoder MFT genuinely **is** registered — `MFT_ENUM_FLAG_HARDWARE`-filtered
      enumeration finds `"NVIDIA AV1 Encoder MFT"` and (listed twice) `"Intel® Hardware
      Accelerated AV1 Encoder MFT"`. Unfiltered (`MFT_ENUM_FLAG_SORTANDFILTER`-only, no
      `HARDWARE` flag — the flag set `open_cpu`'s CPU-upload path uses) finds **zero** AV1
      MFTs; HEVC/VP9 both have a software Store-extension encoder
      (`HEVCVideoExtensionEncoder`/`VP9VideoExtensionEncoder`) in that same unfiltered set,
      AV1 has none. Net effect: `open_hevc_av1_vp9_cpu_or_skip`'s AV1 branch still gets
      `Unsupported` (no non-hardware-flagged MFT to enumerate), and
      `open_hevc_av1_vp9_dx11_or_skip`'s AV1 branch finds the hardware MFT but still fails
      with `EncodeError::Backend` further downstream (D3D11-aware/type-negotiation stage) —
      same pre-existing failure class already observed there for HEVC/VP9 DX11 on this host,
      not a new bug and out of this ADR's scope to fix. So the `av1C` fix above stays
      sans-io-unit-tested-only on this host; the extended `open_hevc_av1_vp9_*_or_skip` av1C
      assertions are live but never yet exercised end-to-end here. This refines (does not
      contradict) the earlier H.264-only "no encode HW MFT on either GPU" wiki finding — that
      finding was about H.264 specifically, not "no HW MFTs for any codec".

### 1b — Umbrella (optional)

- [ ] `mediaway-codec` re-exports encoder (+ decoder when ready) for one-line app deps


### 2 — Web

- [x] Add `mediaway-encoder-web`
- [x] WebCodecs `VideoEncoder` / `AudioEncoder` (CPU path)
- [x] `GPUTexture` → encode Zero-Copy (via WebGPU-backed `OffscreenCanvas`; see caveat below)
- [x] Codec-parameterized audio smoke surface (`is_webcodecs_audio_codec_supported`,
      `encode_audio_buffer`), exercised via Opus alongside AAC — adr/web/0001; wasm32
      compile-verified only, no real-browser verification in this environment; no
      `OpusEncoderConfig` knobs (no `web-sys` binding); no Opus fMP4 mux (`iso-bmff` gap)
- [x] GPU-surface encode generalized to HEVC/AV1/VP9 (`is_webgpu_video_codec_supported`,
      `encode_video_frame_from_webgpu_canvas`, `webcodecs_gpu_video_fmp4_smoke_with_codec`;
      H.264 zero-arg entry points kept as thin wrappers) per crate ADR
      `adr/web/0001-webgpu-multi-codec-video-encode.md` — **wasm32 compile-verified only**, no
      real browser runtime available in this environment; HEVC's Annex-B-vs-length-prefixed
      NAL framing for `iso-bmff`'s `hvc1` fMP4 sample entry is explicitly **unverified** (see
      ADR §2 and `docs/ai/wiki/encode/web-gpu-frame.md`)

### 3 — Linux

- [x] Add `mediaway-encoder-linux`
- [x] VA-API H.264 CPU-upload encode (`cros-libva`; Constrained Baseline, CQP, all-IDR) —
      **zero real-hardware verification**, see crate ADR-0001
- [x] VA-API H.264 single-forward-reference P-frame GOP (`gop_size` finally read by this
      backend, real `frame_num`/reference-picture-list wiring, ported from
      `mediaway-encoder::vulkan::h264_gop::GopState`) — **ADR-0002 implemented**, capability-gated
      on `VAConfigAttribEncMaxRefFrames`; still zero real-hardware verification (WSL2
      check/clippy/test-verified only)
- [x] VA-API HEVC Main profile single-forward-reference P-frame GOP (`vaapi/hevc.rs`/
      `hevc_gop.rs`, `GopState` ported verbatim from `mediaway-encoder::vulkan::
      hevc_gop::GopState`; fresh `EncSequenceParameterBufferHEVC`/`EncPictureParameterBufferHEVC`/
      `EncSliceParameterBufferHEVC` construction — VA-API's own HEVC encode buffers have no
      `StdVideoH265*`-equivalent field set, the driver synthesizes VPS/SPS/PPS itself) —
      **ADR-0003 implemented**, `VaapiVideoEncoder` enum dispatch (H264/Hevc/Vp9, no `Box<dyn>`);
      still zero real-hardware verification (WSL2 check/clippy/test-verified only)
- [ ] VA-API HEVC low-power entrypoint (`VAEntrypointEncSliceLP`) fallback — deferred, ADR-0003
      § Scope
- [ ] VA-API AV1 encode — **blocked**, `cros-libva` 0.0.13 has no packed-header buffer type for
      the app-hand-constructed `frame_header_obu()` bytes AV1 encode requires; design-only, see
      `adr/linux/0005-vaapi-av1-key-frame-and-inter-gop.md`
- [x] VA-API VP9 `KEY_FRAME`-only baseline + single-forward-reference `INTER_FRAME` GOP
      (`VaapiVp9Encoder`, plain `cros-libva` `EncSequenceParameterBufferVP9`/
      `EncPictureParameterBufferVP9` field bags — **not blocked**, unlike the AV1 sibling above;
      3-step entrypoint probe ladder `EncSlice` → `EncPicture` → `EncSliceLP`; new
      `vp9_gop::GopState` 2-slot physical ping-pong; `linux/vaapi/mod.rs` gained this backend's
      first multi-codec **encoder** dispatch enum) per **ADR-0004 implemented** — WSL2
      check/clippy/test-verified only, **zero real-hardware verification**; real-world VP9 VA-API
      *encode* driver support is narrow (`FFmpeg`'s own source names only the i965 classic driver
      as working — VP9 *decode* is broadly supported, encode is not), so this is a
      compile/test-verified-only addition even more than this crate's other VA-API backends, not
      an expected-to-work-on-most-hardware one
- [ ] Vulkan Video encode (alternative/complement to VA-API)
- [ ] GPU buffer Zero-Copy where supported (DMA-BUF surface import)

### 4 — Other

- [ ] `mediaway-encoder::apple` / `mediaway-encoder::android` / `mediaway-encoder::amf` modules
      (ADR-0021 `#[cfg]`-gated, not separate crates) as scheduled
  - [x] AMD AMF: `mediaway-encoder::amf` implemented (`shiguredo_amf`-backed H.264 CPU-upload
        encode, `Encoder`/`EncodeHandler` callback→poll bridge via `Arc<Mutex<VecDeque<_>>>` —
        `EncodeHandler` is `Send + 'static` and its callback runs on `shiguredo_amf`'s own
        internal worker thread, confirmed against real source) per
        `adr/amf/0002-amf-linux-shiguredo-amf-h264-cpu-upload.md` (**Accepted**), superseding the
        earlier `adr/amf/0001` deferral now that the workspace MSRV bump (`docs/adr/0023`) cleared
        the hard blocker. `x86_64`-Linux-only (`shiguredo_amf`'s own platform limit).
        Compile-verified for real on Linux `x86_64` via WSL2 (`cargo check` + `cargo clippy` +
        `cargo test`, including the `AMF_PLANE_TYPE`/`amf_pts`/`amf_size` types this ADR had
        flagged unconfirmed — resolved against real crate source fetched during implementation).
        **Zero real AMD hardware/driver available** — ships 🆗 (compiles, compile-verified on
        Linux `x86_64`, zero hardware verification), never ✅, matching VA-API/Android/Apple. Not
        wired into `auto`/`capability` yet.
    - [x] HEVC + AV1 codec dispatch added per
          `adr/amf/0003-amf-linux-hevc-av1-codec-dispatch.md` (**Accepted, implemented**) —
          `shiguredo_amf`'s own `CodecConfig` already had first-class `Hevc`/`Av1` variants, so
          this was a dispatch widening (`is_supported_video_codec` + a small `codec_config_for`
          match in `session.rs`), not new plumbing; also fixed a real (if previously latent)
          `stream_info_from` codec-hardcode bug along the way. VP9 stays unsupported —
          `shiguredo_amf` has no `CodecConfig` variant for it, a real ceiling of the dependency,
          not a Mediaway restriction. Same compile-only, zero-real-AMD-hardware verification
          posture as H.264 above — still 🆗, never ✅.
  - [x] Android: `mediaway-encoder::android` implemented (NDK `AMediaCodec` via the `ndk`
        crate, H.264 CPU-upload only) per `adr/android/0001-ndk-amediacodec-h264-cpu-upload.md`
        (**Accepted**) — **zero compile verification as authored**, no Android NDK in this dev
        environment; `android` CI job (`.github/workflows/ci.yml`) added in the same PR as the
        first real gate, ahead of hardware verification
  - [x] Apple: `mediaway-encoder::apple` implemented (`VTCompressionSession` via `objc2-*`,
        H.264 CPU-upload only, single module for macOS+iOS) per
        `adr/apple/0001-videotoolbox-h264-cpu-upload.md` (**Accepted**) — **zero compile
        verification as authored**, no Apple SDK/Xcode reachable in this dev environment
        (harder than Android's NDK-only gap: cannot legally cross-compile Apple code outside
        macOS); `apple-macos`/`apple-ios` CI jobs (`.github/workflows/ci.yml`) added in the same
        PR as the first real gate, ahead of hardware verification. Per-packet `is_keyframe` is
        an approximation (`gop_size <= 1 || packet_index == 0`) — real
        `kCMSampleAttachmentKey_NotSync` reading deferred, see ADR-0001 § Implementation notes.
