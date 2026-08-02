# External standards (URL + digest + local cache)

**Do not** paste full ISO/ITU/MPEG/W3C/RFC text into the wiki, specs, or ADRs.

| In the repo (English, short) | Not in the repo |
|------------------------------|-----------------|
| Stable **URL** (or DOI) | Full PDF / HTML dump of the standard |
| **BLAKE3** of the referenced file bytes ([`docs/standards/registry.toml`](../standards/registry.toml)) | Re-hosted copyrighted specification bodies |
| Mediaway-specific notes | `local/standards/**` downloads (gitignored) |

## Digest rule

1. Every cached standard file has an entry in [`docs/standards/registry.toml`](../standards/registry.toml).
2. `blake3` is the lowercase hex digest of the **exact file bytes** under `local/standards/<id>/<filename>`.
3. On fetch or before trusting a local copy, **recompute BLAKE3 and compare** to the registry.
4. Mismatch → stop; do not use the file until the human confirms a URL/edition change and updates the registry.
5. Empty `blake3` means “not pinned yet” — after a lawful first copy, print the digest and commit it to the registry.

Algorithm matches test-media: **BLAKE3** ([`testing.md`](testing.md)).

## Agent workflow

1. Look up `id` / URL / expected `blake3` in the registry (and the short Mediaway note, e.g. [`iso_14496_12_isobmff.md`](../spec/iso_14496_12_isobmff.md)).
2. Cache layout:

```text
local/standards/<id>/
  source.url          ← exact URL used (must match registry url unless human overrides)
  source.ua           ← User-Agent used for the fetch (human vs ai-agent)
  <filename>          ← body
  NOTES.md            ← optional scratch (not SSOT)
```

3. Prefer: `bun tools/scripts/fetch-standard.ts [--ai-agent] <id>` (fetch when allowed + verify).
4. **User-Agent / provenance (this script only):**
   - Default (no flag): human maintainer — `…; human-maintainer)`
   - `--ai-agent`: coding agent — `…; ai-coding-agent)`  
   Agents **must** pass `--ai-agent` when using **this** fetch tool. The chosen UA is written to `local/standards/<id>/source.ua`.
   - **Do not** set a Mediaway / `Mediaway-standards-fetch` `User-Agent` on any other HTTP client, browser, curl, or ad-hoc download. That UA identifies **this maintainers’ script** only — not the Mediaway product and not generic agent browsing.
5. Prefer free official sources (IETF RFC, W3C/WHATWG, Khronos, …).
6. **Paywalled ISO/IEC:** `paywalled = true` → no auto-download. Human places a lawful file, then:
   `bun tools/scripts/fetch-standard.ts pin <id>` to compute digest and show the registry line to commit.
7. Never `git add` `local/standards/`.

## Related

- [`local-workspace.md`](local-workspace.md) · [`local/README.md`](../../local/README.md)
- Script: [`tools/scripts/fetch-standard.ts`](../../tools/scripts/fetch-standard.ts)
