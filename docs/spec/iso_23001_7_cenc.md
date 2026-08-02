# ISO/IEC 23001-7 — Mediaway notes (Common Encryption)

**Do not paste the standard text here.** Canonical catalog URL and local digest pin:

| | |
|--|--|
| Registry id | `iso-23001-7` |
| Catalog | https://www.iso.org/standard/68042.html |
| Cache | `local/standards/iso-23001-7/` (gitignored) |

## Mediaway scope

| In scope (ClearKey) | Out of scope |
|---------------------|--------------|
| Sample AES under `cenc` (AES-128-CTR) | Widevine / FairPlay / PlayReady CDMs |
| Subsample clear/protected ranges | Key servers / license exchange |
| Caller-supplied 128-bit content key (+ optional KID) | Opaque vendor crypto libs |

Container boxes (`tenc`, `senc`, `saiz`, `saio`, `sinf`, …) are parsed in `iso-bmff`. Sample keystream application lives in **`iso-cenc`**. Shared demux decrypt hook: `mediaway-container::DemuxDecrypt`.

Policy: [`docs/adr/0011-clearkey-cenc.md`](../adr/0011-clearkey-cenc.md).
