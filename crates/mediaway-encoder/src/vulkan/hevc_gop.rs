//! Multi-frame GOP state for HEVC P-frame encode (ADR-0002) — HEVC sibling
//! of [`super::h264_gop`]. Same pure-Rust, no-FFI, no-`unsafe` DPB ring
//! buffer and single-forward-reference `decide` state machine; simpler than
//! H.264's because HEVC's `StdVideoEncodeH265ReferenceInfo` carries only
//! `PicOrderCntVal` (no `FrameNum` equivalent — POC is the sole ordering
//! value this crate signals) and `StdVideoEncodeH265PictureInfo` has no
//! `idr_pic_id` field to sequence.
//!
//! [`WORKSPACE_DPB_CAP`](super::h264_gop::WORKSPACE_DPB_CAP) is reused
//! directly from `h264_gop` rather than redeclared here — it is a
//! genuinely codec-agnostic constant (DPB slot count headroom, not tied to
//! any H.264-specific syntax), matching how `session_command.rs`'s
//! upload/barrier/readback helpers are shared code, not duplicated, in
//! `session_command_hevc.rs`.

#![allow(
    clippy::redundant_pub_crate,
    reason = "workspace `unreachable_pub` policy (Cargo.toml) wants `pub(crate)` here; \
              clippy::pedantic's redundant_pub_crate disagrees for private modules — the \
              two lints are mutually exclusive for this shape, workspace policy wins"
)]

use super::h264_gop::WORKSPACE_DPB_CAP;

/// `log2_max_pic_order_cnt_lsb_minus4` this crate's SPS uses whenever GOP
/// encode is active — `12` (HEVC's spec-legal maximum, giving
/// `log2_max_pic_order_cnt_lsb = 16`) rather than Stage 1's `0`
/// (`log2_max_pic_order_cnt_lsb = 4`, wraps at 16). `PicOrderCntVal` resets
/// to `0` at every IDR (see [`GopState::decide`]), so it only needs to stay
/// unwrapped across one GOP — same reasoning as
/// [`h264_gop::LOG2_MAX_FRAME_NUM_MINUS4`](super::h264_gop::LOG2_MAX_FRAME_NUM_MINUS4),
/// picking the widest legal field sidesteps implementing H.265 §8.3.1's
/// `PicOrderCntMsb` wraparound derivation (irrelevant to this crate's
/// single-forward-reference design) for any `gop_size` up to 65536 frames.
pub(crate) const LOG2_MAX_PIC_ORDER_CNT_LSB_MINUS4: u8 = 12;

/// One populated DPB slot: the `PicOrderCntVal`/picture-type of the picture
/// currently stored there — enough to rebuild the
/// `StdVideoEncodeH265ReferenceInfo` a later frame's read of this slot needs
/// ([`hevc_params::build_reference_info`](super::hevc_params::build_reference_info)).
#[derive(Debug, Clone, Copy)]
pub(crate) struct DpbSlot {
    pub(crate) poc: i32,
    pub(crate) is_idr: bool,
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

/// Caller-requested frame kind for [`GopState::decide`] — mirrors
/// [`h264_gop::FrameRequest`](super::h264_gop::FrameRequest); duplicated
/// rather than shared since the two `GopState`s are otherwise independent
/// types (see this module's doc for why they aren't unified).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "ForceIdr is a documented hook, not wired by any caller yet"
)]
pub(crate) enum FrameRequest {
    Auto,
    ForceIdr,
}

/// One frame's resolved encode plan: IDR vs P, `PicOrderCntVal`, and which
/// DPB slot to write into / (optionally) read as the sole L0 reference.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FrameDecision {
    pub(crate) is_idr: bool,
    pub(crate) poc: i32,
    pub(crate) setup_slot: usize,
    pub(crate) reference: Option<(usize, DpbSlot)>,
}

/// Per-session forward-only prediction state — HEVC sibling of
/// [`h264_gop::GopState`](super::h264_gop::GopState). Owned by the caller
/// (`VulkanVideoEncoder`), mutated in place per frame — no per-frame
/// allocation.
#[derive(Debug)]
pub(crate) struct GopState {
    gop_size: u32,
    frames_since_idr: u32,
    poc: i32,
    dpb: Dpb,
    last_written: Option<usize>,
}

impl GopState {
    /// `gop_size == 1` reproduces Stage 1's all-IDR behavior exactly: every
    /// `decide` call returns `is_idr: true, poc: 0, reference: None` — the
    /// same values [`super::hevc_params::build_idr_picture_info`] already
    /// hardcodes, so routing every HEVC frame through this state machine
    /// (regardless of `gop_size`) keeps default output byte-identical.
    pub(crate) fn new(gop_size: u32) -> Self {
        Self {
            gop_size,
            frames_since_idr: 0,
            poc: 0,
            dpb: Dpb::default(),
            last_written: None,
        }
    }

    pub(crate) fn decide(&mut self, request: FrameRequest) -> FrameDecision {
        let is_idr = matches!(request, FrameRequest::ForceIdr)
            || self.frames_since_idr == 0
            || self.frames_since_idr >= self.gop_size;
        if is_idr {
            // An IDR picture marks every prior reference "unused for
            // reference" per H.265 semantics — this crate's DPB mirrors
            // that by discarding all tracked slot state and restarting the
            // ring.
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

        // Bookkeeping for the frame just decided: record it into its own
        // setup slot so a *future* `decide` call can read it back as
        // `reference`, then advance every counter for the *next* call.
        self.dpb.slots[setup_slot] = Some(DpbSlot { poc, is_idr });
        self.dpb.next_slot = (setup_slot + 1) % WORKSPACE_DPB_CAP;
        self.last_written = Some(setup_slot);
        self.poc += 1;
        self.frames_since_idr += 1;

        decision
    }
}
