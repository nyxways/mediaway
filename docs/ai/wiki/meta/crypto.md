# ClearKey CENC (`iso-cenc`)

Canonical: [ADR-0011](../../../adr/0011-clearkey-cenc.md) · [spec note](../../../spec/iso_23001_7_cenc.md) · naming [ADR-0012](../../../adr/0012-unprefixed-reusable-cores.md).

- Sans-io sample crypto — no file I/O; no DRM CDM.
- Unprefixed reusable core (`iso-cenc`).
- Stage 1: `cenc` AES-128-CTR; subsample clear ranges do not advance CTR.
- Block cipher: RustCrypto `aes`; CTR rules owned in-crate.
- `iso-bmff` parses `tenc`/`senc`; `DemuxDecrypt` on `mediaway-container`.
- FATE ClearKey: `12345678901234567890123456789012` (hex).
