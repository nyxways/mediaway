# Decoder crate scaffold

- Path: `crates/mediaway-decoder` (**facade**); ADRs 0001–0002
- Windows: `mediaway-decoder-windows` — WMF H.264, two output paths:
  - `VideoOutputPreference::ZeroCopyGpu` — HW decoder MFT + DXGI Zero-Copy out (`ID3D11Device` required)
  - `VideoOutputPreference::CpuFramesOk` — software (sync) H.264 decoder MFT, no GPU/device
    manager at all; NV12 copied straight from the MFT's system-memory buffer
    (`IMF2DBuffer::Lock2D` stride-aware, contiguous `Lock` fallback) — see `wmf/cpu.rs`
- Verified end-to-end via `tests/cpu_roundtrip.rs`: encodes through
  `mediaway-encoder-windows`'s CPU-upload H.264 path, decodes through the CPU path above —
  no committed media, no GPU needed for either side (no mux/demux; Annex-B throughout)
- AVCC↔Annex-B: demuxed MP4 samples are AVCC length-prefixed with an
  `AVCDecoderConfigurationRecord` `extra_data`, but WMF's decoder MFTs expect Annex-B.
  `open_dx11`/`open_cpu` detect this (`iso_bmff::bitstream::avc::parse_avc_decoder_config`)
  and convert both `extra_data` and every packet payload before reaching the MFT — see
  ADR-0001 and `mediaway`'s `tests/trim_and_splice_windows.rs` for the real
  encode→mux→demux→decode proof that found this gap
- README: OS · GPU / D3D11 decode 🆗; CPU path 🆗 (SW, no HW offload)
- Windows HEVC/AV1/VP9 CPU decode: `wmf/video_cpu.rs`'s `WmfMultiCodecCpuDecoder` — real,
  tested, and wired into `WindowsVideoDecoder::open`'s public dispatch (`CodecKind::Hevc |
  Av1 | Vp9` routes here; only H.264 gets its own DX11 Zero-Copy branch). HEVC/VP9 verified
  real via
  encode→decode round trip (`HEVCVideoExtension`/`VP9VideoExtensionDecoder` MFTs). AV1
  decoder MFT (`AV1VideoExtension`) is real and accepts a genuine system-`ffmpeg`/`libaom`
  AV1 stream, but only ever proposes `MFVideoFormat_AYUV` output for it, never NV12 — this
  crate is NV12-only by design, so that's `DecodeError::Unsupported`, not a bug. Real bug
  found+fixed here: after `MF_E_TRANSFORM_STREAM_CHANGE`, these extension decoder MFTs reject
  a caller-reconstructed output type (unlike H.264's inbox decoder, where that path is never
  actually exercised) — must re-negotiate via the MFT's own `GetOutputAvailableType`/
  `SetOutputType` instead. See `mediaway-decoder-windows/docs/roadmap.md`.
- Linux: `mediaway-decoder-linux` — VA-API H.264 CPU-output decode, **IDR pictures only**
  (no DPB / reference management), own SPS/PPS/slice parser reusing `mediaway_sw::h264`'s
  bit reader/NAL framing — see [platform/linux-decode](../platform/linux-decode.md) and
  that crate's ADR-0001. **Zero real-hardware verification** in the session that authored it.
- Web: `mediaway-decoder-web` — WebCodecs `VideoDecoder`, `EncodedVideoChunk` in →
  `VideoFrame` out, luma-plane CPU readback via `copyTo`. No facade trait impl (async API).
  See [web-video-decode](web-video-decode.md).
- Later: demuxer→decode→encode integration smoke, Annex-B/AVCC policy for demuxer-sourced
  streams (open on both Windows and Linux backends)

## Capability probe (2026-07-31)

`mediaway_decoder::capability::{DecodeSupport, DecodeUnavailable}` +
`mediaway_pipeline::platform::decoder_support(codec)` — mirrors the encoder facade's
probe (`mediaway-encoder` ADR-0004), but reports a single `DecodeSupport` per codec
instead of a `Vec` of rows: unlike encode, decode has exactly one implementation per
platform today (`mediaway-decoder-vulkan` is real but unwired — see
[vulkan-decode](../platform/vulkan-decode.md) — so there's no second backend to
enumerate). Implemented as a tiny throwaway 64×64 open (empty `extra_data` is tolerated
by WMF/VA-API at open time), same live-probe cost trade-off as `encoder_support`.
Compile-time OS filtering: non-Windows/non-Linux targets return `NotImplemented`
without touching anything.
