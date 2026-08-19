//! VP9's persistent 8-slot reference-frame shadow table — this ADR's own central finding (see
//! `adr/linux/0004-vaapi-vp9-key-frame-and-inter-decode.md` § "A second, independent finding")
//! that VP9 decode needs only a **two-field**-per-slot metadata table (`width`/`height`, for
//! `frame_size_with_refs()`), not AV1's twelve-field `RefFrameWidth[]`/`RefFrameType[]`/... set.
//!
//! Logical VP9 reference slots (`0..8`, spec-fixed, never stream-derived) are decoupled from
//! *physical* VA-API `Surface` pool indices: a single `refresh_frame_flags` byte can legally name
//! several logical slots for one freshly-decoded picture at once (this crate's own encoder
//! sibling's ping-pong output does exactly this — `refresh_frame_flags = (1 << slot) | 0xfc`
//! refreshes 7 of the 8 logical slots on every `INTER_FRAME`). Rather than physically duplicating
//! surface content, multiple logical slots may share the same physical pool index — this table
//! stores that pool index per logical slot (a plain, `Copy`, sans-io integer), while
//! `super::Vp9Pipeline::surfaces` (VA-API-calling code, not this module) owns the actual
//! `Surface` objects.
//!
//! [`RefTable::free_pool_index`]'s pigeonhole guarantee (`POOL_SIZE = VP9_REF_SLOTS + 1`) is
//! pure, sans-io logic — unit-tested here without any VA-API device.

#![forbid(unsafe_code)]

/// VP9's spec-fixed logical reference-frame slot count — never stream-derived (unlike H.264's
/// `max_num_ref_frames`), so this crate needs no per-session sizing computation at all.
pub(super) const VP9_REF_SLOTS: usize = 8;

/// Physical `Surface` pool capacity: `VP9_REF_SLOTS + 1`. By pigeonhole, at most `VP9_REF_SLOTS`
/// distinct pool indices can be referenced across the 8 logical slots at any one time, so a pool
/// of `VP9_REF_SLOTS + 1` always has at least one index free for the current decode target —
/// see [`RefTable::free_pool_index`].
pub(super) const POOL_SIZE: usize = VP9_REF_SLOTS + 1;

/// One logical slot's shadow metadata: which physical pool index currently backs it, and that
/// picture's own coded `width`/`height` (needed by `frame_size_with_refs()` — this crate's own
/// finding that VP9 decode needs nothing more per slot than this).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RefEntry {
    pub(super) pool_index: usize,
    pub(super) width: u32,
    pub(super) height: u32,
}

/// The persistent 8-logical-slot table. Owned by the caller (`VaapiVp9Decoder`), outlives any
/// single `Vp9Pipeline` the same way `VaapiAv1Decoder::seq` outlives `Av1Pipeline` — a fresh
/// session with no pipeline yet still has a well-defined (all-empty) table, so an `INTER_FRAME`
/// arriving before any `KEY_FRAME` fails cleanly at `frame_size_with_refs()`'s own lookup rather
/// than needing a separate "have we seen a key frame" flag.
#[derive(Debug, Clone, Copy)]
pub(super) struct RefTable {
    entries: [Option<RefEntry>; VP9_REF_SLOTS],
}

impl RefTable {
    pub(super) const fn new() -> Self {
        Self {
            entries: [None; VP9_REF_SLOTS],
        }
    }

    /// The full entry at logical slot `slot` (`0..8`), if occupied. Out-of-range `slot` (should
    /// never happen — `ref_frame_idx` is a spec `f(3)` field, always `0..8`) returns `None`
    /// rather than panicking.
    pub(super) fn get(&self, slot: usize) -> Option<RefEntry> {
        self.entries.get(slot).copied().flatten()
    }

    /// `(width, height)` at logical slot `slot`, for `frame_size_with_refs()`.
    pub(super) fn size(&self, slot: usize) -> Option<(u32, u32)> {
        self.get(slot).map(|e| (e.width, e.height))
    }

    /// A physical pool index in `0..POOL_SIZE` not currently referenced by any logical slot —
    /// always exists by the pigeonhole argument in [`POOL_SIZE`]'s doc comment. The `0` fallback
    /// is unreachable in practice, kept defensive rather than panicking.
    pub(super) fn free_pool_index(&self) -> usize {
        (0..POOL_SIZE)
            .find(|candidate| {
                !self
                    .entries
                    .iter()
                    .flatten()
                    .any(|e| e.pool_index == *candidate)
            })
            .unwrap_or(0)
    }

    /// Point every logical slot named by `refresh_frame_flags` (bit `i` = slot `i`) at
    /// `pool_index`/`width`/`height` — the real, spec-legal multi-slot-aliasing case this
    /// table's own doc comment describes.
    pub(super) fn refresh(
        &mut self,
        refresh_frame_flags: u8,
        pool_index: usize,
        width: u32,
        height: u32,
    ) {
        for (i, entry) in self.entries.iter_mut().enumerate() {
            if refresh_frame_flags & (1 << i) != 0 {
                *entry = Some(RefEntry {
                    pool_index,
                    width,
                    height,
                });
            }
        }
    }
}

#[cfg(test)]
#[path = "ref_table_tests.rs"]
mod tests;
