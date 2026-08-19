//! AV1's fixed 8-slot reference-frame-buffer array (AV1 spec § 7.20).
//!
//! **Not** a port of `dpb.rs`'s H.264/HEVC-shaped `Dpb`/`DpbSlot` (no sliding
//! window, no `frame_num`/POC, no RPS — see `adr/vulkan/0002`'s "Reference-
//! model design" section for why). This round's `KEY_FRAME`-only scope never
//! *reads* a reference slot (every `referenceNameSlotIndices` entry is always
//! `-1`), so this type tracks only Vulkan-level slot occupancy +
//! outstanding-Zero-Copy-handle bookkeeping — structurally mirroring
//! `dpb.rs`'s `SlotOutstanding` contract, not sharing its code (that code is
//! coupled to H.264-specific fields this type has no use for). A future
//! general-GOP increment adds real `order_hint`/`frame_type`-per-slot
//! tracking here (mirroring `mediaway-encoder::vulkan::av1_gop::DpbSlot`'s
//! decode-shaped sibling) — deliberately not built this round, since nothing
//! in a `KEY_FRAME`-only decoder ever reads it.
//!
//! The **physical** Vulkan DPB image this round needs is small regardless of
//! AV1's "8 reference names" logical space — a key-frame-only stream needs
//! only 1-2 physical `dpb_slot_count` array layers (current picture +
//! optionally one prior), not 8 (`adr/vulkan/0002`'s own § Reference-model
//! design finding) — this type's own `occupied`/`outstanding` arrays are
//! sized to the real physical slot count the caller allocates, not a fixed
//! `[bool; 8]`.

#![forbid(unsafe_code)]

use thiserror::Error;

/// Errors from [`Av1RefSlots`]'s slot bookkeeping — mirrors `dpb::DpbError`'s
/// shape exactly (same two failure modes: an out-of-range index, or a slot
/// whose Zero-Copy handle a caller still holds).
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Av1RefSlotsError {
    /// The caller tried to recycle a slot whose GPU image a caller still
    /// holds via an outstanding Zero-Copy
    /// [`mediaway_common::GpuBufferHandle::Vulkan`] handle — same contract as
    /// [`crate::vulkan::dpb::DpbError::SlotOutstanding`].
    #[error("AV1 DPB slot {index} still has an outstanding Zero-Copy handle")]
    SlotOutstanding {
        /// The slot index that could not be recycled.
        index: usize,
    },
    /// No free slot exists and no occupied slot is available to evict (every
    /// occupied slot has an outstanding handle, or the pool has zero
    /// capacity).
    #[error("no free AV1 DPB slot available (capacity {capacity})")]
    NoFreeSlot {
        /// Total slot capacity at the time of the failed allocation.
        capacity: usize,
    },
    /// `index` is not a valid slot index for this pool's capacity.
    #[error("AV1 DPB slot index {index} out of range (capacity {capacity})")]
    InvalidSlotIndex {
        /// The out-of-range index that was requested.
        index: usize,
        /// Total pool capacity.
        capacity: usize,
    },
}

/// Vulkan-level DPB slot occupancy + outstanding-Zero-Copy-handle
/// bookkeeping for this crate's `KEY_FRAME`-only AV1 decode session.
///
/// Every `KEY_FRAME` clears the whole pool before allocating its own slot
/// (AV1 spec: a key frame's `refresh_frame_flags == 0xFF` conceptually
/// refreshes every one of AV1's 8 reference names, and this round never
/// reads a prior reference either way) — `clear_all`/`allocate_slot` mirror
/// `dpb::Dpb`'s identical IDR-clears-everything shape used by
/// `decoder_hevc.rs`.
pub(crate) struct Av1RefSlots {
    occupied: Vec<bool>,
    outstanding: Vec<bool>,
}

impl Av1RefSlots {
    /// Allocate a pool with `capacity` slots, clamped to at least `1` (a
    /// decoder always needs at least one slot for the current picture).
    #[must_use]
    pub(crate) fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            occupied: vec![false; capacity],
            outstanding: vec![false; capacity],
        }
    }

    const fn check_index(&self, index: usize) -> Result<(), Av1RefSlotsError> {
        if index >= self.occupied.len() {
            Err(Av1RefSlotsError::InvalidSlotIndex {
                index,
                capacity: self.occupied.len(),
            })
        } else {
            Ok(())
        }
    }

    /// Marks `index` as holding an outstanding Zero-Copy handle — call when
    /// handing a [`mediaway_common::GpuBufferHandle::Vulkan`] out to a
    /// caller (see `zero_copy.rs`).
    ///
    /// # Errors
    /// Returns [`Av1RefSlotsError::InvalidSlotIndex`] if `index` is out of
    /// range.
    pub(crate) fn mark_outstanding(&mut self, index: usize) -> Result<(), Av1RefSlotsError> {
        self.check_index(index)?;
        self.outstanding[index] = true;
        Ok(())
    }

    /// Evicts every occupied slot — a `KEY_FRAME` conceptually refreshes
    /// every AV1 reference name, so this round clears the whole pool before
    /// each decode, mirroring `dpb::Dpb::clear_all`'s identical IDR
    /// contract.
    ///
    /// # Errors
    /// Returns [`Av1RefSlotsError::SlotOutstanding`] for the first occupied
    /// slot that still has an outstanding Zero-Copy handle — the caller must
    /// drain pending frames first.
    pub(crate) fn clear_all(&mut self) -> Result<(), Av1RefSlotsError> {
        for index in 0..self.occupied.len() {
            if self.occupied[index] {
                if self.outstanding[index] {
                    return Err(Av1RefSlotsError::SlotOutstanding { index });
                }
                self.occupied[index] = false;
            }
        }
        Ok(())
    }

    /// Allocate a slot index for a new picture to decode into: the first
    /// unoccupied slot, or — since [`Av1RefSlots::clear_all`] already runs
    /// before every `KEY_FRAME` in this round's scope — slot `0` is always
    /// free by the time this is called in practice; kept as a real search
    /// (not a hardcoded `0`) so a future caller that skips `clear_all` still
    /// gets correct, not silently-overwriting, behavior.
    ///
    /// # Errors
    /// Returns [`Av1RefSlotsError::NoFreeSlot`] if every slot is occupied.
    pub(crate) fn allocate_slot(&mut self) -> Result<usize, Av1RefSlotsError> {
        let index = self.occupied.iter().position(|&occupied| !occupied).ok_or(
            Av1RefSlotsError::NoFreeSlot {
                capacity: self.occupied.len(),
            },
        )?;
        self.occupied[index] = true;
        Ok(index)
    }
}

#[cfg(test)]
#[path = "av1_refs_tests.rs"]
mod tests;
