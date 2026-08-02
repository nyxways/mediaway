# ADR-0011: ClearKey CENC in-product (no DRM CDM)

- **Status**: Accepted
- **Date**: 2026-07-27
- **Deciders**: @dev-nyxie (+ agent)

## Context

fMP4 / CMAF often use ISO Common Encryption ([ISO/IEC 23001-7](https://www.iso.org/standard/68042.html)). Tests and ClearKey workflows need sample decrypt when the caller supplies keys.

Shipping Widevine / FairPlay / PlayReady CDMs would pull foreign license systems and opaque binaries into the product identity.

## Decision

> Mediaway implements **ClearKey ISO CENC** via unprefixed sans-io crate **`iso-cenc`**. Callers supply content keys (and optional KID). **No DRM CDM** in the product graph.

- **Standard-driven:** sample crypto per ISO/IEC 23001-7; container boxes (`tenc`/`senc`/…) stay in format crate
- **Sans-io:** encrypt/decrypt on byte slices + subsample layout; no file/network I/O
- **Stage 1:** `cenc` (AES-128-CTR) first; `cens`/`cbc1`/`cbcs` later as needed
- **Layout:** `iso-cenc` (ADR-0012); `iso-bmff` depends on it; facade `DemuxDecrypt` on `mediaway-container`
- **Deps:** RustCrypto `aes` only; CTR counter rules owned by `iso-cenc`

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| Depend on `sheathe-crypto` / packager stacks | Extra media model; not our SSOT |
| Full DRM CDMs | Out of product identity and license scope |

## Consequences

- Clear crypto ownership; testable against FATE ClearKey samples; we maintain CTR/subsample edge cases

## References

- `iso-23001-7` in [`docs/standards/registry.toml`](../standards/registry.toml)
- `crates/iso-cenc/`, `crates/mediaway-container/`
- ADR-0002, ADR-0003, ADR-0012
