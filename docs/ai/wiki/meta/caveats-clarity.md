# Caveats & code clarity

Canonical: [`docs/spec/caveats-and-clarity.md`](../../../spec/caveats-and-clarity.md) · ADR-0006.

- Costly compat paths (GL→DX copy, CPU readback, PCM `Vec` copy sold as ⚡, …) → rustdoc + honest names + catalog
- No silent slow defaults; **⚡** = no payload memcpy ([marks](../zero-copy/marks.md))
- Public API understandable from code + rustdoc alone
- Reviewer: missing caveat/rustdoc on costly or public API → **Blocking**
