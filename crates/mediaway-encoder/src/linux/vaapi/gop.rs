//! Multi-frame GOP state for VA-API H.264 P-frame encode (ADR-0002) — a
//! small, pure-Rust DPB ring buffer and `frame_num`/`PicOrderCnt` bookkeeping
//! state machine, ported **verbatim** from
//! [`crate::vulkan::h264_gop`](../../vulkan/h264_gop.rs) (see
//! `adr/linux/0002-vaapi-h264-p-frame-gop.md`'s porting table). That source
//! is already GPU-API-agnostic — "No Vulkan FFI, no `unsafe`" — so this port
//! needed no adaptation surgery beyond `pub(super)` visibility.
//!
//! Deliberately narrow, matching ADR-0002's scope: single forward reference
//! only (`RefPicList0[0]`, never `RefPicList1`), no B-frames (permanent
//! non-goal), no long-term references, no reference-list reordering.
//!
//! No `cros_libva` types or VA-API calls anywhere in this file — every
//! function operates on plain data, so it is unit-testable without any VA-API
//! device (see `gop_tests.rs`).

#![forbid(unsafe_code)]

/// Fixed-capacity DPB ring size — enough for one active forward reference
/// plus in-flight pipelining headroom. A bare array beats `SmallVec` here
/// since there is no heap-spill case to avoid (ported reasoning, see
/// `vulkan::h264_gop::WORKSPACE_DPB_CAP`'s own doc). Happens to equal this
/// crate's pre-existing `SURFACE_POOL_SIZE` (`video.rs`), so the physical
/// surface pool needs no size change, only a new selection strategy.
pub(super) const WORKSPACE_DPB_CAP: usize = 4;

/// `log2_max_frame_num_minus4` this crate's SPS uses whenever GOP encode is
/// active (`gop_size > 1` and the driver supports it) — `12` (H.264's
/// spec-legal maximum, `log2_max_frame_num = 16`) rather than this crate's
/// existing all-IDR value of `4`. `frame_num` resets to `0` at every IDR (see
/// [`GopState::decide`]), so it only needs to stay unwrapped across one GOP;
/// picking the widest legal field sidesteps implementing H.264 §8.2.4.1's
/// `FrameNumWrap` arithmetic for any `gop_size` up to 65536 frames. Only
/// applied when GOP mode is actually active; `gop_size <= 1` keeps this
/// crate's existing SPS value (`4`) unchanged.
pub(super) const LOG2_MAX_FRAME_NUM_MINUS4: u8 = 12;

/// One populated DPB slot: the `frame_num`/`PicOrderCnt`/picture-type of the
/// picture currently stored there.
#[derive(Debug, Clone, Copy)]
pub(super) struct DpbSlot {
    pub(super) frame_num: u32,
    pub(super) poc: i32,
    #[allow(
        dead_code,
        reason = "verbatim-ported field (vulkan/h264_gop.rs); unlike Vulkan's PictureH264 \
                  analogue, VA-API's reference-list entries (video.rs::reference_picture_h264) \
                  carry no primary_pic_type, so a referenced slot's is_idr is unread in \
                  production code — exercised by gop_tests.rs's reference-construction \
                  assertions, kept for parity with the porting source's shape"
    )]
    pub(super) is_idr: bool,
}

/// Fixed-capacity ring of DPB slots (see [`WORKSPACE_DPB_CAP`]).
#[derive(Debug, Clone, Copy)]
struct Dpb {
    slots: [Option<DpbSlot>; WORKSPACE_DPB_CAP],
    next_slot: usize,
}

impl Default for Dpb {
    fn default() -> Self {
        Self {
            slots: [None; WORKSPACE_DPB_CAP],
            next_slot: 0,
        }
    }
}

/// Caller-requested frame kind for [`GopState::decide`]. `Auto` is this
/// ADR's only wired case; `ForceIdr` is a hook for a future out-of-band IDR
/// request, left unimplemented by any caller this pass, mirroring the
/// porting source's own disposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "ForceIdr is a documented hook, not wired by any caller yet"
)]
pub(super) enum FrameRequest {
    Auto,
    ForceIdr,
}

/// One frame's resolved encode plan: IDR vs P, `frame_num`/`PicOrderCnt`, and
/// which DPB slot to write into / (optionally) read as the sole L0
/// reference.
#[derive(Debug, Clone, Copy)]
pub(super) struct FrameDecision {
    pub(super) is_idr: bool,
    pub(super) frame_num: u32,
    pub(super) poc: i32,
    /// Only meaningful when `is_idr` — mirrors this crate's pre-ADR-0002
    /// hardcoded `idr_pic_id: 0`, byte-identical sequencing preserved (see
    /// `GopState::decide`'s doc).
    pub(super) idr_pic_id: u16,
    pub(super) setup_slot: usize,
    pub(super) reference: Option<(usize, DpbSlot)>,
}

/// Per-session forward-only prediction state (ADR-0002's `GopState` sketch).
/// Owned by the caller (`VaapiVideoEncoder`), mutated in place per frame —
/// no per-frame allocation.
#[derive(Debug)]
pub(super) struct GopState {
    gop_size: u32,
    frames_since_idr: u32,
    frame_num: u32,
    idr_counter: u16,
    dpb: Dpb,
    last_written: Option<usize>,
}

impl GopState {
    /// `gop_size == 1` reproduces this crate's pre-ADR-0002 all-IDR behavior
    /// exactly: every `decide` call returns `is_idr: true`, `frame_num: 0`,
    /// `poc: 0`, `reference: None`, and `idr_pic_id` increments `0, 1, 2,
    /// ...` — so routing every H.264 frame through this state machine
    /// (regardless of `gop_size`) keeps default output byte-identical.
    pub(super) fn new(gop_size: u32) -> Self {
        Self {
            gop_size,
            frames_since_idr: 0,
            frame_num: 0,
            idr_counter: 0,
            dpb: Dpb::default(),
            last_written: None,
        }
    }

    pub(super) fn decide(&mut self, request: FrameRequest) -> FrameDecision {
        let is_idr = matches!(request, FrameRequest::ForceIdr)
            || self.frames_since_idr == 0
            || self.frames_since_idr >= self.gop_size;
        if is_idr {
            // An IDR picture marks every prior reference "unused for
            // reference" per H.264 semantics — this crate's DPB mirrors that
            // by discarding all tracked slot state and restarting the ring.
            self.frame_num = 0;
            self.frames_since_idr = 0;
            self.dpb = Dpb::default();
            self.last_written = None;
        }

        let setup_slot = self.dpb.next_slot;
        let reference = if is_idr {
            None
        } else {
            self.last_written
                .and_then(|slot| self.dpb.slots[slot].map(|dpb_slot| (slot, dpb_slot)))
        };
        let poc = 2 * i32::try_from(self.frame_num).unwrap_or(i32::MAX);
        let idr_pic_id = self.idr_counter;

        let decision = FrameDecision {
            is_idr,
            frame_num: self.frame_num,
            poc,
            idr_pic_id,
            setup_slot,
            reference,
        };

        // Bookkeeping for the frame just decided: record it into its own
        // setup slot so a *future* `decide` call can read it back as
        // `reference`, then advance every counter for the *next* call.
        self.dpb.slots[setup_slot] = Some(DpbSlot {
            frame_num: self.frame_num,
            poc,
            is_idr,
        });
        self.dpb.next_slot = (setup_slot + 1) % WORKSPACE_DPB_CAP;
        self.last_written = Some(setup_slot);
        if is_idr {
            self.idr_counter = self.idr_counter.wrapping_add(1);
        }
        self.frame_num = (self.frame_num + 1) % (1u32 << (LOG2_MAX_FRAME_NUM_MINUS4 + 4));
        self.frames_since_idr += 1;

        decision
    }
}

#[cfg(test)]
#[path = "gop_tests.rs"]
mod tests;
