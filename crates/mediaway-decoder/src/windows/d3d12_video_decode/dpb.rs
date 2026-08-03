//! Fixed-size DPB slot pool (ADR-0002's "texture array" DPB mode) — no encode-side
//! precedent (every encoder in this workspace is all-intra, no DPB). Generic over `M`,
//! the per-codec reference metadata (H.264's [`super::h264_refs::H264RefMeta`] today;
//! HEVC/AV1 metadata later), so this file needs no change when those codecs land.
//!
//! Owns exactly one `ID3D12Resource` NV12 texture array; each subresource is one DPB
//! slot. Enforces the ADR's **bounded-handle backpressure contract**: a slot the
//! decoder wants to reuse but whose Zero-Copy `GpuBufferHandle` a caller may still be
//! holding is never silently overwritten — [`DpbPool::acquire_free_slot`] /
//! [`SlotTable::evict`] fail loudly instead (`DecodeError::Backend`, see ADR-0002 Open
//! Question #2 for why not a dedicated variant).
//!
//! The slot bookkeeping ([`SlotTable`]) is a separate, D3D12-free type from
//! [`DpbPool`] (which just adds the owned `ID3D12Resource`) specifically so it can be
//! unit-tested without a real device/texture — see `dpb_tests.rs`.

use crate::DecodeError;
use windows::Win32::Graphics::Direct3D12::ID3D12Resource;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotState {
    Free,
    /// Holds a decoded picture — either still needed for output, still an active
    /// reference, or both.
    Occupied,
}

struct Slot<M> {
    state: SlotState,
    /// `true` while a caller may still be holding a live Zero-Copy `GpuBufferHandle`
    /// referencing this slot's subresource (see [`SlotTable::mark_handle_outstanding`] /
    /// [`SlotTable::release_handle`]).
    handle_outstanding: bool,
    /// `true` while the picture is an active reference (independent of
    /// `handle_outstanding` — a slot can be a live reference with no caller-held
    /// handle at all, the common case for pure decode-and-reference pictures never
    /// requested as Zero-Copy output).
    is_reference: bool,
    meta: Option<M>,
}

impl<M> Default for Slot<M> {
    fn default() -> Self {
        Self {
            state: SlotState::Free,
            handle_outstanding: false,
            is_reference: false,
            meta: None,
        }
    }
}

/// Pure slot-lifecycle bookkeeping for a fixed-size DPB, with no D3D12 dependency —
/// unit-testable directly (`dpb_tests.rs`).
pub(super) struct SlotTable<M> {
    slots: Vec<Slot<M>>,
}

impl<M: Copy> SlotTable<M> {
    pub(super) fn new(num_slots: u32) -> Self {
        let mut slots = Vec::with_capacity(num_slots as usize);
        slots.resize_with(num_slots as usize, Slot::default);
        Self { slots }
    }

    pub(super) fn num_slots(&self) -> u32 {
        u32::try_from(self.slots.len()).unwrap_or(0)
    }

    /// Find a free (not occupied) slot. Callers needing more capacity than is
    /// currently free must evict a reference first via [`SlotTable::evict`].
    ///
    /// # Errors
    ///
    /// [`DecodeError::Backend`] when no slot is free — the DPB is sized with headroom
    /// (`max_num_ref_frames + caller_headroom`, see `setup.rs`/ADR-0002) so ordinary
    /// playback should not hit this; a real hit means every slot is either an active
    /// reference or has a caller-outstanding Zero-Copy handle.
    pub(super) fn acquire_free_slot(&mut self) -> Result<u32, DecodeError> {
        let index = self
            .slots
            .iter()
            .position(|s| s.state == SlotState::Free)
            .ok_or(DecodeError::Backend)?;
        self.slots[index].state = SlotState::Occupied;
        Ok(u32::try_from(index).unwrap_or(0))
    }

    /// Evict slot `index` from reference use, freeing it for reuse — **unless** a
    /// caller may still hold a live Zero-Copy handle to it, in which case this fails
    /// loudly rather than silently reusing memory the caller may be reading (ADR-0002's
    /// bounded-handle backpressure contract).
    ///
    /// # Errors
    ///
    /// [`DecodeError::Backend`] when `slots[index].handle_outstanding` — the caller
    /// must release its `GpuBufferHandle` (consume/copy the frame) before decode can
    /// proceed further.
    pub(super) fn evict(&mut self, index: u32) -> Result<(), DecodeError> {
        let slot = self
            .slots
            .get_mut(index as usize)
            .ok_or(DecodeError::Backend)?;
        if slot.handle_outstanding {
            return Err(DecodeError::Backend);
        }
        slot.state = SlotState::Free;
        slot.is_reference = false;
        slot.meta = None;
        Ok(())
    }

    pub(super) fn mark_reference(&mut self, index: u32, meta: M) {
        if let Some(slot) = self.slots.get_mut(index as usize) {
            slot.is_reference = true;
            slot.meta = Some(meta);
        }
    }

    /// Mark slot `index` as no longer needed once decode/output are both done with it
    /// (not currently a reference and no outstanding handle) — frees it immediately
    /// instead of waiting for the sliding-window process to evict it later.
    pub(super) fn release_if_unused(&mut self, index: u32) {
        if let Some(slot) = self.slots.get_mut(index as usize) {
            if !slot.is_reference && !slot.handle_outstanding {
                slot.state = SlotState::Free;
            }
        }
    }

    pub(super) fn mark_handle_outstanding(&mut self, index: u32) {
        if let Some(slot) = self.slots.get_mut(index as usize) {
            slot.handle_outstanding = true;
        }
    }

    /// Release a previously-outstanding Zero-Copy handle for slot `index` (the caller
    /// has consumed/copied the frame and no longer needs it). Frees the slot
    /// immediately if it is also not an active reference.
    pub(super) fn release_handle(&mut self, index: u32) {
        if let Some(slot) = self.slots.get_mut(index as usize) {
            slot.handle_outstanding = false;
            if !slot.is_reference {
                slot.state = SlotState::Free;
            }
        }
    }

    pub(super) fn is_free(&self, index: u32) -> bool {
        self.slots
            .get(index as usize)
            .is_some_and(|s| s.state == SlotState::Free)
    }

    /// Every slot currently marked as an active reference, paired with its metadata —
    /// input to `h264_refs`' `RefPicList0`/`RefPicList1` construction and sliding-window
    /// eviction decision.
    pub(super) fn references(&self) -> Vec<(u32, M)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                if s.is_reference {
                    s.meta.map(|m| (u32::try_from(i).unwrap_or(0), m))
                } else {
                    None
                }
            })
            .collect()
    }
}

/// Fixed-size DPB slot pool: [`SlotTable`] bookkeeping plus the one owned NV12
/// texture-array `ID3D12Resource` those slot indices address as subresources.
pub(super) struct DpbPool<M> {
    texture: ID3D12Resource,
    table: SlotTable<M>,
}

impl<M: Copy> DpbPool<M> {
    pub(super) fn new(texture: ID3D12Resource, num_slots: u32) -> Self {
        Self {
            texture,
            table: SlotTable::new(num_slots),
        }
    }

    pub(super) const fn texture(&self) -> &ID3D12Resource {
        &self.texture
    }

    pub(super) const fn table(&self) -> &SlotTable<M> {
        &self.table
    }

    pub(super) const fn table_mut(&mut self) -> &mut SlotTable<M> {
        &mut self.table
    }
}

#[cfg(test)]
#[path = "dpb_tests.rs"]
mod tests;
