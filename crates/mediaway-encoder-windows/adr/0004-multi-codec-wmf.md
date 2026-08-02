# ADR-0004: Multi-codec WMF video (H.264 / HEVC / AV1 / VP9)

- **Status**: Accepted
- **Date**: 2026-07-28
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `mediaway-encoder-windows`

## Context

Stage 1 wired H.264 only (`CLSID_MSH264EncoderMFT` + HW H.264 MFT). Product tables list HEVC / AV1 / VP9 as planned. Media Foundation exposes subtypes (`MFVideoFormat_HEVC`, `AV1`, `VP90`) and hardware encoder MFTs on capable GPUs — without vendor SDKs.

## Decision

> Parameterize the existing encode session by [`CodecKind`](../../mediaway-common):
>
> 1. Map codec → MF output subtype (`wmf/codec.rs`).
> 2. **CPU:** H.264 keeps the inbox sync MFT; HEVC/AV1/VP9 use `MFTEnumEx` (any match). Often `Unsupported` when no soft MFT exists — honest failure.
> 3. **Zero-Copy:** hardware `MFTEnumEx` + DXGI for all four codecs when a D3D11-aware HW encoder is present.
> 4. Session type renamed conceptually to `WmfVideoEncoder` (same `WindowsVideoEncoder::open` surface).

ProRes / Opus video remain out of scope. VendorHw (NVENC API, …) stays a separate README axis.

## Consequences

- README OS·GPU / D3D11 cells for HEVC/AV1/VP9 become prototype (`🆗`) when open+push works on a machine; otherwise stay skip/`🛠️` until proven.
- Decode crate mirrors the same subtype map for HW decode open.

## References

- ADR-0003 (DX11 Zero-Copy) · root README codec tables
