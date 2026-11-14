# ADR-0002: NVENC live H.264 session (D3D11 Zero-Copy input, own SDK 13.0 bindings)

- **Status**: Accepted
- **Date**: 2026-11-15
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder` (`nvenc` module)

## Context

A downstream game streams one window (Windows Graphics Capture, D3D11 BGRA textures) from a
host to 16+ viewers over lossy multi-hop relays. It needs: GPU texture input with no
readback, the lowest encoder latency, a flat bitrate (CBR, small VBV), **keyframes only on
demand** (a viewer joins or reports loss), the bitrate changed at runtime, and optionally
intra refresh instead of keyframe spikes. H.264 was chosen over AV1 by the user for decoder
reach (hardware H.264 decode exists on every target GPU; see `mediaway-decoder`
`adr/windows/0007`).

The existing `NvencVideoEncoder` ([ADR-0001](0001-nvenc-vendor-backend.md)) uses the
crates.io `nvenc` 0.1 safe layer, which cannot do this: it has no `nvEncReconfigureEncoder`,
no `encodePicFlags` (forced IDR, SPS/PPS output), keeps the rate-control VBV fields and the
H.264 config (IDR period, `repeatSPSPPS`, intra refresh) private, and panics / prints on some
error paths (`unwrap` in `Drop`, `println!`).

## Decision

> A second NVENC session type, `nvenc::NvencLiveEncoder` (+ `NvencLiveConfig`), over a
> **hand-written subset of the NVENC SDK 13.0 API** (`nvenc/live/ffi.rs`); the CPU-upload
> `NvencVideoEncoder` keeps the `nvenc` crate.

- **Bindings:** only the structs / functions used, each ending in its reserved tail so the size
  equals C. Sizes and offsets were dumped from the SDK 13.0 header (`FFmpeg`
  `nv-codec-headers` `n13.0.19.0`, MIT) with `clang-cl` and are pinned by `const` asserts, so a
  layout slip does not compile. The DLL is loaded from System32 at first use and must report
  API ≥ 13.0 (`NvEncodeAPIGetMaxSupportedVersion`), else `EncodeError::Backend`.
- **Input:** caller `ID3D11Texture2D` on the caller's device (BGRA / RGBA / NV12). Registered
  once (`nvEncRegisterResource`, cache of 8 with a COM reference each so an address cannot be
  reused), mapped / unmapped per frame. NVENC converts RGB to YUV; the SPS VUI says BT.709
  limited range (measured: decoded luma matches a BT.709 reference at 47 dB, BT.601 at 28 dB).
- **Coding:** H.264 High, preset P4 (configurable P1..P7), `ULTRA_LOW_LATENCY` tuning
  (or `LOW_LATENCY`), CBR, VBV = one frame by default, no B-frames, lookahead off, zero
  reorder delay, `gopLength = idrPeriod = NVENC_INFINITE_GOPLENGTH`, `repeatSPSPPS`.
- **Control:** `request_keyframe()` → next picture `FORCEIDR | OUTPUT_SPSPPS`;
  `set_bitrate()` → `nvEncReconfigureEncoder` (no reset, no IDR; VBV keeps its length in
  frames; `Unsupported` without `NV_ENC_CAPS_SUPPORT_DYN_BITRATE_CHANGE`); `resize()` →
  reconfigure with reset + IDR up to `max_width` / `max_height`; optional
  `IntraRefresh { period_frames, duration_frames }` with recovery-point SEI.
- **Mode:** synchronous (one bitstream buffer; the lock waits), so one input gives one packet.
  `Send`, not `Sync`.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Fork / patch the `nvenc` crate | Its safe layer would need most of these changes anyway, plus its panics removed; vendoring a fork costs more than ~600 lines of pinned FFI. |
| `bindgen` the SDK header | Brings libclang back into the build (the reason for ADR-0007). |
| Extend `NvencVideoEncoder` | Mixing two binding layers and two input models in one session; the live knobs do not fit `VideoEncoderConfig` (on-demand IDR, runtime bitrate). |
| Async mode (`enableEncodeAsync`, completion events) | Lets a caller pipeline frames; for one stream at 60 fps the sync lock is simpler and the measured encode time is far below a frame. |

## Consequences

### Positive

- Zero-Copy capture → encode; measured on an RTX 4090 at 1080p60 / 10 Mbps
  (`machine_id: ad-hoc`, see the crate roadmap): median ~3 ms submit→packet in release.
- Exactly one packet per frame and keyframes only when asked: what a relay fan-out needs.

### Negative / Trade-offs

- Two NVENC binding layers in one crate. The hand-written one is pinned to SDK 13.0; a driver
  older than R570 (API < 13.0) is `Backend`.
- Windows / D3D11 only; D3D12 input (fence points) is not wired.
- The registration cache keeps up to 8 caller textures alive until evicted, resized or dropped.

## References

- [ADR-0001](0001-nvenc-vendor-backend.md), [crate ADR-0007](../0007-quicksync-feature.md)
- NVENC Video Encoder API Programming Guide (SDK 13.0) — low-latency / intra refresh sections
- `mediaway-decoder` `adr/windows/0007-wmf-live-h264-decode.md` (the receiving side)
