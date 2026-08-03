//! Sans-io DPB (decoded picture buffer) slot bookkeeping — H.264 today, shaped
//! so HEVC/AV1 can reuse the same slot machinery later (see `adr/0001`'s
//! "DPB / reference-management design sketch").
//!
//! No Vulkan/GPU calls anywhere in this file — every function operates on
//! plain data, so it is unit-testable without any device (see
//! `dpb_tests.rs`), mirroring how `mediaway_sw::h264`'s own bitstream parsing
//! is tested independent of any decode/encode hardware.
//!
//! Two related but distinct concerns:
//! - **Reference-picture-set bookkeeping** ([`DpbSlot`]/[`Dpb`]): which
//!   previously-decoded pictures are current references, sized once from the
//!   parsed SPS's `max_num_ref_frames`, evicted by sliding-window order
//!   (ITU-T H.264 § 8.2.5.3).
//! - **Zero-Copy handle backpressure**: a slot whose GPU image a caller still
//!   holds (via [`mediaway_common::GpuBufferHandle::Vulkan`]) must never be
//!   silently recycled — [`Dpb::insert`]/[`Dpb::evict`] fail loudly
//!   ([`DpbError::SlotOutstanding`]) instead, the same "fail loudly, never
//!   silently overwrite" contract the D3D12 sibling ADR uses for its own DPB.

#![forbid(unsafe_code)]

use thiserror::Error;

/// H.264's spec-defined maximum DPB slot count.
///
/// `max_num_ref_frames` can be signaled up to this many; this crate's own
/// "current picture" slot is tracked separately by the caller — see
/// `session.rs`'s `max_dpb_slots` sizing, which adds one for the picture
/// currently being decoded.
pub const H264_MAX_DPB_SLOTS: usize = 16;

/// One decoded-picture-buffer slot's reference-management bookkeeping.
///
/// Deliberately holds only the fields H.264's sliding-window reference
/// process (ITU-T H.264 § 8.2.4 / § 8.2.5.3) and Vulkan's
/// `StdVideoDecodeH264ReferenceInfo` need — no pixel data, no Vulkan handle
/// (those live in `session.rs`'s per-slot GPU image array, indexed the same
/// way).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DpbSlot {
    /// `frame_num` as signaled in this picture's slice header.
    pub frame_num: u32,
    /// `FrameNumWrap` (ITU-T H.264 § 8.2.4.1) — `frame_num`, or
    /// `frame_num - MaxFrameNum` when `frame_num` is greater than the
    /// current picture's `frame_num` (wrapped around `log2_max_frame_num`).
    /// Sliding-window eviction picks the occupied reference slot with the
    /// smallest `frame_num_wrap`.
    pub frame_num_wrap: i32,
    /// `PicOrderCnt` — this crate supports `pic_order_cnt_type == 0` only
    /// (see `h264_params.rs`), so top/bottom field order counts are always
    /// equal for the frame pictures this crate decodes; a single `i32` is
    /// enough.
    pub pic_order_cnt: i32,
    /// Whether this picture is currently used as a short-term reference
    /// (`used_for_reference` in the spec's DPB bookkeeping). `false` for a
    /// non-reference picture kept in the DPB only for output reordering —
    /// this crate does not reorder output (see `h264_slice.rs`), so a
    /// `false` slot here is only ever the still-being-decoded current
    /// picture before its own reference status is known.
    pub used_for_reference: bool,
}

impl DpbSlot {
    /// Construct a new short-term reference slot.
    #[must_use]
    pub const fn new_reference(frame_num: u32, frame_num_wrap: i32, pic_order_cnt: i32) -> Self {
        Self {
            frame_num,
            frame_num_wrap,
            pic_order_cnt,
            used_for_reference: true,
        }
    }
}

/// `FrameNumWrap` (ITU-T H.264 § 8.2.4.1, short-term picture case).
///
/// `frame_num` unchanged if not greater than the current picture's
/// `frame_num`, otherwise wrapped by subtracting `MaxFrameNum`
/// (`1 << log2_max_frame_num`).
#[must_use]
#[allow(
    clippy::cast_possible_wrap,
    reason = "frame_num/max_frame_num are H.264 frame_num values, bounded by \
              log2_max_frame_num (<= 16 bits per the spec's own field width) — never close to \
              i32::MAX, so the wrap this lint warns about is unreachable in practice"
)]
pub const fn compute_frame_num_wrap(
    frame_num: u32,
    current_frame_num: u32,
    max_frame_num: u32,
) -> i32 {
    if frame_num > current_frame_num {
        frame_num as i32 - max_frame_num as i32
    } else {
        frame_num as i32
    }
}

/// Errors from [`Dpb`]'s slot bookkeeping.
///
/// Crate-internal (not `crate::DecodeError` — see `session.rs`'s
/// `VulkanDecodeError`, which wraps this via `#[from]` and is itself mapped
/// to `DecodeError` at `decoder.rs`'s public boundary).
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DpbError {
    /// The caller tried to insert into or evict a slot whose GPU image a
    /// caller still holds via an outstanding Zero-Copy
    /// [`mediaway_common::GpuBufferHandle::Vulkan`] handle — recycling it now
    /// would silently invalidate that handle's contents underneath the
    /// caller. Never overwritten silently; the caller must recycle the
    /// handle (drop the frame / poll further) before this slot can be reused.
    #[error("DPB slot {index} still has an outstanding Zero-Copy handle")]
    SlotOutstanding {
        /// The slot index that could not be recycled.
        index: usize,
    },
    /// No free slot exists and no occupied reference slot is available to
    /// evict (e.g. every occupied slot has an outstanding handle, or the DPB
    /// has zero capacity).
    #[error("no free DPB slot available (capacity {capacity})")]
    NoFreeSlot {
        /// Total DPB capacity at the time of the failed allocation.
        capacity: usize,
    },
    /// `index` is not a valid slot index for this DPB's capacity.
    #[error("DPB slot index {index} out of range (capacity {capacity})")]
    InvalidSlotIndex {
        /// The out-of-range index that was requested.
        index: usize,
        /// Total DPB capacity.
        capacity: usize,
    },
}

/// Fixed-capacity DPB slot array, sized once from the parsed SPS's
/// `max_num_ref_frames` (see [`Dpb::new`]) — a `Vec` allocated once at
/// session-open time, not per-frame.
pub struct Dpb {
    slots: Vec<Option<DpbSlot>>,
    /// Parallel array: `true` while a caller holds an outstanding Zero-Copy
    /// handle into that slot's image. Kept separate from `DpbSlot` itself so
    /// an evicted-but-still-held slot can still report `SlotOutstanding`
    /// (the reference bookkeeping is gone, but the handle contract is not).
    outstanding: Vec<bool>,
}

impl Dpb {
    /// Allocate a DPB with `capacity` slots, clamped to `1..=H264_MAX_DPB_SLOTS`
    /// (a decoder always needs at least one slot for the current picture,
    /// even when the stream signals `max_num_ref_frames == 0`).
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.clamp(1, H264_MAX_DPB_SLOTS);
        Self {
            slots: vec![None; capacity],
            outstanding: vec![false; capacity],
        }
    }

    /// Total slot count.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }

    /// The slot at `index`, if occupied.
    #[must_use]
    pub fn slot(&self, index: usize) -> Option<&DpbSlot> {
        self.slots.get(index).and_then(Option::as_ref)
    }

    /// Whether `index` currently has an outstanding Zero-Copy handle.
    #[must_use]
    pub fn is_outstanding(&self, index: usize) -> bool {
        self.outstanding.get(index).copied().unwrap_or(false)
    }

    /// First unoccupied slot index, if any.
    #[must_use]
    pub fn free_slot_index(&self) -> Option<usize> {
        self.slots.iter().position(Option::is_none)
    }

    /// Every occupied slot, in slot-index order.
    pub fn occupied_slots(&self) -> impl Iterator<Item = (usize, &DpbSlot)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| slot.as_ref().map(|slot| (index, slot)))
    }

    fn check_index(&self, index: usize) -> Result<(), DpbError> {
        if index >= self.slots.len() {
            Err(DpbError::InvalidSlotIndex {
                index,
                capacity: self.slots.len(),
            })
        } else {
            Ok(())
        }
    }

    /// Marks `index` as holding an outstanding Zero-Copy handle — call when
    /// handing a [`mediaway_common::GpuBufferHandle::Vulkan`] out to a caller
    /// (see `zero_copy.rs`).
    ///
    /// # Errors
    /// Returns [`DpbError::InvalidSlotIndex`] if `index` is out of range.
    pub fn mark_outstanding(&mut self, index: usize) -> Result<(), DpbError> {
        self.check_index(index)?;
        self.outstanding[index] = true;
        Ok(())
    }

    /// Clears `index`'s outstanding-handle mark — call once the caller has
    /// recycled the corresponding `VideoFrame` (the next `push_packet`/
    /// `poll_frame`/`flush` call that would otherwise want to reuse this
    /// slot, per `crate::VideoDecoder`'s documented handle
    /// lifetime).
    ///
    /// # Errors
    /// Returns [`DpbError::InvalidSlotIndex`] if `index` is out of range.
    pub fn clear_outstanding(&mut self, index: usize) -> Result<(), DpbError> {
        self.check_index(index)?;
        self.outstanding[index] = false;
        Ok(())
    }

    /// Insert `slot` at `index`, marking it occupied.
    ///
    /// # Errors
    /// Returns [`DpbError::InvalidSlotIndex`] if `index` is out of range, or
    /// [`DpbError::SlotOutstanding`] if `index` still has an outstanding
    /// Zero-Copy handle — never silently overwritten.
    pub fn insert(&mut self, index: usize, slot: DpbSlot) -> Result<(), DpbError> {
        self.check_index(index)?;
        if self.outstanding[index] {
            return Err(DpbError::SlotOutstanding { index });
        }
        self.slots[index] = Some(slot);
        Ok(())
    }

    /// Evict (clear) the slot at `index`.
    ///
    /// # Errors
    /// Returns [`DpbError::InvalidSlotIndex`] if `index` is out of range, or
    /// [`DpbError::SlotOutstanding`] if `index` still has an outstanding
    /// Zero-Copy handle.
    pub fn evict(&mut self, index: usize) -> Result<(), DpbError> {
        self.check_index(index)?;
        if self.outstanding[index] {
            return Err(DpbError::SlotOutstanding { index });
        }
        self.slots[index] = None;
        Ok(())
    }

    /// The occupied short-term-reference slot with the smallest
    /// `frame_num_wrap` — the sliding-window eviction target (ITU-T H.264
    /// § 8.2.5.3) when the DPB is full and a new reference picture needs a
    /// slot.
    #[must_use]
    pub fn sliding_window_evict_target(&self) -> Option<usize> {
        self.occupied_slots()
            .filter(|(_, slot)| slot.used_for_reference)
            .min_by_key(|(_, slot)| slot.frame_num_wrap)
            .map(|(index, _)| index)
    }

    /// Recomputes every occupied slot's `frame_num_wrap` relative to
    /// `current_frame_num` (ITU-T H.264 § 8.2.4.1).
    ///
    /// `FrameNumWrap` is defined **relative to the picture currently being
    /// decoded**, not fixed when a slot was inserted — the caller must call
    /// this once per picture (with that picture's own `frame_num`) before
    /// [`Dpb::allocate_slot`]'s sliding-window eviction or
    /// [`crate::vulkan::h264_slice::default_ref_pic_list0`]'s list-building, or both
    /// would silently use stale values once `frame_num` wraps around
    /// `MaxFrameNum`.
    pub fn refresh_frame_num_wraps(&mut self, current_frame_num: u32, max_frame_num: u32) {
        for slot in self.slots.iter_mut().flatten() {
            slot.frame_num_wrap =
                compute_frame_num_wrap(slot.frame_num, current_frame_num, max_frame_num);
        }
    }

    /// Evicts every occupied slot — an IDR picture empties the whole DPB
    /// (ITU-T H.264 § 8.2.5.1: an IDR access unit marks every prior reference
    /// picture "unused for reference").
    ///
    /// # Errors
    /// Returns [`DpbError::SlotOutstanding`] for the first occupied slot that
    /// still has an outstanding Zero-Copy handle — the caller must drain
    /// pending frames first; slots already evicted before the failing one
    /// stay evicted (partial progress, not rolled back — matches
    /// [`Dpb::evict`]'s own single-slot failure contract).
    pub fn clear_all(&mut self) -> Result<(), DpbError> {
        let occupied: Vec<usize> = self.occupied_slots().map(|(index, _)| index).collect();
        for index in occupied {
            self.evict(index)?;
        }
        Ok(())
    }

    /// Allocate a slot index for a new picture to decode into: reuses a free
    /// slot if one exists, otherwise forces sliding-window eviction of the
    /// oldest short-term reference.
    ///
    /// # Errors
    /// Returns [`DpbError::SlotOutstanding`] if the eviction target still has
    /// an outstanding Zero-Copy handle (the caller must drain pending frames
    /// before this DPB can accept another picture), or
    /// [`DpbError::NoFreeSlot`] if every slot is empty of references and
    /// still somehow unavailable (unreachable in practice — a zero-capacity
    /// DPB is rejected by [`Dpb::new`]'s clamp — kept as an explicit error
    /// rather than a `panic!`/`unwrap`).
    pub fn allocate_slot(&mut self) -> Result<usize, DpbError> {
        if let Some(index) = self.free_slot_index() {
            return Ok(index);
        }
        let index = self
            .sliding_window_evict_target()
            .ok_or(DpbError::NoFreeSlot {
                capacity: self.slots.len(),
            })?;
        self.evict(index)?;
        Ok(index)
    }
}

#[cfg(test)]
#[path = "dpb_tests.rs"]
mod tests;
