# ADR-0001: Standard-based ClearKey `cenc` (owned CTR)

- **Status**: Accepted
- **Date**: 2026-07-27
- **Deciders**: @dev-nyxie (+ agent)
- **Crate**: `iso-cenc`

## Context

Third-party CENC crates either pull full packagers, encrypt-only APIs, or native
decryptors. Mediaway needs a small sans-io sample API with an explicit decrypt
path and correct subsample counter rules. The crate is unprefixed (ADR-0012 naming v1).

## Decision

> Implement ISO/IEC 23001-7 **`cenc`** ourselves: RustCrypto `aes` for the block
> cipher only; this crate owns CTR counter construction and subsample walking.

1. Public `decrypt_cenc` / `encrypt_cenc` (CTR is an involution).
2. `Subsample { clear_bytes, protected_bytes }`; empty list = whole buffer protected.
3. Pattern schemes rejected until Stage 2.
4. No I/O; no KID lookup service — caller binds key material.

## Alternatives Considered

| Alternative | Why not |
|-------------|---------|
| `sheathe-crypto` dependency | Not our SSOT; encrypt-only surface |
| Hand-rolled CTR that ignores subsample rules | Counter advancement bugs on clear ranges |

## Consequences

### Positive

- Correct ClearKey decrypt for FATE / CMAF ClearKey paths

### Negative / Trade-offs

- We maintain scheme matrix growth (`cens`/`cbcs`) ourselves

## References

- workspace ADR-0011 · ADR-0012
- `docs/spec/iso_23001_7_cenc.md`
