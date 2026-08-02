# ISOBMFF (ISO/IEC 14496-12) — Mediaway notes

**External standard:** do not vendor the full text here.  
Policy: [`docs/conventions/external-standards.md`](../conventions/external-standards.md).

## Official references (URL)

Registry digests: [`docs/standards/registry.toml`](../standards/registry.toml) (`iso-14496-12`, `iso-14496-15`).

| Doc | URL |
|-----|-----|
| ISO/IEC 14496-12 (ISOBMFF) catalog | https://www.iso.org/standard/83102.html |
| ISO/IEC 14496-15 (NAL structured video in ISOBMFF) | https://www.iso.org/standard/74429.html |

Agents: after a lawful local PDF is present, `bun tools/scripts/fetch-standard.ts pin iso-14496-12` and commit the printed `blake3` into the registry. Verify with `… verify iso-14496-12`. Do not commit downloads under `local/standards/`.

## Mediaway implementation crib (not the ISO text)

Fragmented MP4 shape we target in sans-io mux:

```text
ftyp
moov
  mvhd
  trak… (tkhd / mdia / minf / stbl…)
  mvex / trex…
moof / traf / tfhd / tfdt / trun   (batched samples)
mdat
```

- Big-endian `size` + 4-char `type`; `size == 1` → 64-bit largesize
- `mvex` required for fragmented MP4
- Timescales via `Rational` / track `mdhd`

Details live in `iso-bmff` / `mediaway-container` rustdoc and crate ADRs.
