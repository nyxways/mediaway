//! Multi-frame GOP state for VA-API HEVC P-frame encode (ADR-0003) — HEVC sibling of
//! [`super::gop`]. Ported **verbatim** from
//! [`crate::vulkan::hevc_gop`](../../vulkan/hevc_gop.rs) (see
//! `adr/linux/0003-vaapi-hevc-p-frame-gop.md`'s porting table). That source is already
//! GPU-API-agnostic — no Vulkan FFI, no `unsafe` — so this port needed no adaptation surgery
//! beyond `pub(super)` visibility, mirroring `gop.rs`'s identical H.264 port (ADR-0002).
//!
//! Simpler than [`super::gop::GopState`]: HEVC's single ordering value is `PicOrderCntVal` (no
//! `frame_num`/`FrameNumWrap` concept), and HEVC pictures carry no `idr_pic_id` — so
//! `FrameDecision`/`GopState` here are smaller structs than their H.264 siblings. `poc` is never
//! wrapped modulo a `MaxPicOrderCntLsb`-style field (unlike `gop.rs`'s `frame_num`) — the porting
//! source (`vulkan/hevc_gop.rs`) never wraps it either, so there is no wraparound case to test.
//!
//! Deliberately narrow, matching ADR-0003's scope: single forward reference only
//! (`RefPicList0[0]`, signaled via a per-picture short-term RPS), no B-frames (permanent
//! non-goal), no long-term references, no reference-list reordering.
//!
//! No `cros_libva` types or VA-API calls anywhere in this file — every function operates on
//! plain data, so it is unit-testable without any VA-API device (see `hevc_gop_tests.rs`).

#![forbid(unsafe_code)]

use super::gop::WORKSPACE_DPB_CAP;

/// One populated DPB slot: the `PicOrderCntVal`/picture-type of the picture currently stored
/// there.
#[derive(Debug, Clone, Copy)]
pub(super) struct DpbSlot {
    pub(super) poc: i32,
    #[allow(
        dead_code,
        reason = "verbatim-ported field (vulkan/hevc_gop.rs); this crate's own reference-list \
                  construction (hevc.rs::reference_picture_hevc) needs only `poc` — kept for \
                  parity with the porting source's shape, mirrors gop.rs::DpbSlot::is_idr's \
                  identical disposition"
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

/// Caller-requested frame kind for [`GopState::decide`]. `Auto` is this ADR's only wired case;
/// `ForceIdr` is a hook for a future out-of-band IDR request, left unimplemented by any caller
/// this pass, mirroring [`super::gop::FrameRequest`]'s identical disposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "ForceIdr is a documented hook, not wired by any caller yet"
)]
pub(super) enum FrameRequest {
    Auto,
    ForceIdr,
}

/// One frame's resolved encode plan: IDR vs P, `PicOrderCntVal`, and which DPB slot to write
/// into / (optionally) read as the sole L0 reference.
#[derive(Debug, Clone, Copy)]
pub(super) struct FrameDecision {
    pub(super) is_idr: bool,
    pub(super) poc: i32,
    pub(super) setup_slot: usize,
    pub(super) reference: Option<(usize, DpbSlot)>,
}

/// Per-session forward-only prediction state (ADR-0003's `GopState` sketch) — HEVC sibling of
/// [`super::gop::GopState`]. Owned by the caller ([`super::hevc::VaapiHevcVideoEncoder`]),
/// mutated in place per frame — no per-frame allocation.
#[derive(Debug)]
pub(super) struct GopState {
    gop_size: u32,
    frames_since_idr: u32,
    poc: i32,
    dpb: Dpb,
    last_written: Option<usize>,
}

impl GopState {
    /// `gop_size == 1` reproduces this crate's pre-ADR-0003 all-IDR behavior exactly: every
    /// `decide` call returns `is_idr: true, poc: 0, reference: None` — so routing every HEVC
    /// frame through this state machine (regardless of `gop_size`) keeps default output
    /// byte-identical.
    pub(super) fn new(gop_size: u32) -> Self {
        Self {
            gop_size,
            frames_since_idr: 0,
            poc: 0,
            dpb: Dpb::default(),
            last_written: None,
        }
    }

    pub(super) fn decide(&mut self, request: FrameRequest) -> FrameDecision {
        let is_idr = matches!(request, FrameRequest::ForceIdr)
            || self.frames_since_idr == 0
            || self.frames_since_idr >= self.gop_size;
        if is_idr {
            // An IDR picture marks every prior reference "unused for reference" per H.265
            // semantics — this crate's DPB mirrors that by discarding all tracked slot state and
            // restarting the ring.
            self.poc = 0;
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
        let poc = self.poc;

        let decision = FrameDecision {
            is_idr,
            poc,
            setup_slot,
            reference,
        };

        // Bookkeeping for the frame just decided: record it into its own setup slot so a
        // *future* `decide` call can read it back as `reference`, then advance every counter for
        // the *next* call.
        self.dpb.slots[setup_slot] = Some(DpbSlot { poc, is_idr });
        self.dpb.next_slot = (setup_slot + 1) % WORKSPACE_DPB_CAP;
        self.last_written = Some(setup_slot);
        self.poc += 1;
        self.frames_since_idr += 1;

        decision
    }
}

#[cfg(test)]
#[path = "hevc_gop_tests.rs"]
mod tests;
