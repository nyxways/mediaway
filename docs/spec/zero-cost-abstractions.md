# Zero-cost abstractions (ZCA)

Mediaway designs Rust APIs so abstractions **compile away** on hot paths: enums, generics, typestate, and concrete ownership — not heap type-erasure by default.

**Not the same as Zero-Copy:** Zero-Copy avoids *moving bytes* (GPU handles, `Bytes` / shared PCM). ZCA avoids *paying for abstraction* (vtable, forced heap). Both matter; see [`gpu-interop.md`](gpu-interop.md), [`vision.md`](vision.md), and [wiki marks](../ai/wiki/zero-copy/marks.md).

Workspace decision: [ADR-0009](../adr/0009-zero-cost-abstractions.md).

## Process — plan before code

For non-trivial Rust (new modules, public types, session state machines, parsers/emitters):

1. **Sketch the shape in chat** (user’s language): ownership, enums vs traits, typestate steps, where bytes live, where allocs happen.
2. Prefer **closed** variant sets (`enum`) over open objects.
3. Only then implement. Trivial typos / one-liners skip this.

Agents: also follow [`AGENTS.md`](../../AGENTS.md) absolute rule on ZCA / `Box`.

## Toolkit (prefer → avoid)

| Prefer | Notes |
|--------|--------|
| `enum` + exhaustive match | Closed ISOBMFF/box/codec sets |
| Generics / monomorphization | Shared write/parse without `dyn` |
| Typestate | Illegal states unrepresentable (`Open` → `Live`) |
| Concrete structs + inherent methods | Inlinable; rustdoc carries contract |
| `&[u8]` / `Bytes` / reused `Vec` | Aligns with alloc discipline |
| `SmallVec<[T; N]>` | When `N` is usually small — see below |
| Traits for **caller** contracts | `Mux` / `Demux` — implementors stay concrete |

| Avoid on hot / sans-io paths | Unless |
|------------------------------|--------|
| `Box<T>` / `Box<[u8]>` | Size / rare cold / measured need + rustdoc |
| `Box<dyn Trait>` / `dyn Trait` | Facade plugin feature or FFI erasure, documented |
| Habitual `Vec` for tiny lists | Prefer `SmallVec` or fixed array |
| “Flexibility” type erasure early | Grow variants; don’t erase first |

## `Box` rules

1. **Default deny** on mux/demux/bitstream/timebase/hot packet·frame pumps.
2. If used: **name why** in rustdoc (or crate ADR). Silent `Box` for “easier API” is a smell.
3. Prefer `enum` of known backends over `dyn Backend` when the set is closed at compile time.
4. Large recursive trees may use heap *nodes* only after alternatives (arena, indices, `SmallVec` children) are considered.

## SmallVec

**Approved** (ADR-0009) for bounded, usually-inline collections.

| Use when | Skip when |
|----------|-----------|
| Typical len ≤ inline `N` (e.g. 4–16) | Length unbounded or usually large |
| Spill to heap is rare but OK | Must never allocate → `arrayvec` / fixed array |
| Replacing tiny `Vec` churn | One-shot cold config → plain `Vec` is fine |

**Examples (candidates, not mandates):** SPS/PPS NAL lists, small track tables, short ISOBMFF child lists, small pending-sample batches.

**Adding the crate:** pin via `[workspace.dependencies]` (`default-features = false` unless needed), `cargo deny`, PR notes deps checklist.

**In-tree:** `iso-bmff` uses `INLINE_TRACKS` / `INLINE_SAMPLES` for tracks, pending fragments, and `trun` rows. Large payloads remain `Vec<u8>`.

Alternatives: `[T; N]` + length, `arrayvec` (no spill), `Vec` (always heap).

## Checklist (before merge of a ZCA-shaped change)

- [ ] Ownership / lifetimes clear from types + rustdoc
- [ ] No new hot-path `Box`/`dyn` without justification
- [ ] Allocs named or reused; tiny lists considered for `SmallVec`
- [ ] Public low-level surface still usable ([`api-layers.md`](api-layers.md))
- [ ] Costly escapes documented ([`caveats-and-clarity.md`](caveats-and-clarity.md))

## Related

- Alloc / clone: [`docs/conventions/code-style.md`](../conventions/code-style.md)
- Sans-IO: [`sans-io.md`](sans-io.md)
- API layers: [`api-layers.md`](api-layers.md)
- Deps: [`docs/conventions/deps-policy.md`](../conventions/deps-policy.md)
