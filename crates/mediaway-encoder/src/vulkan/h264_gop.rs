//! Multi-frame GOP state for H.264 P-frame encode (ADR-0002) — a small,
//! pure-Rust DPB ring buffer and `frame_num`/`PicOrderCnt` bookkeeping state
//! machine layered on top of Stage 1's all-IDR path
//! (`adr/0001-vulkan-video-encode-ash-probe.md`). No Vulkan FFI, no
//! `unsafe` — [`h264_params`](super::h264_params) turns a [`FrameDecision`]
//! into the `StdVideoH264*` structs `vkCmdEncodeVideoKHR` needs.
//!
//! Deliberately narrow, matching ADR-0002's scope: single forward reference
//! only (`RefPicList0[0]`, never `RefPicList1`), no B-frames (permanent
//! non-goal, not just deferred — see the ADR's Context section), no
//! long-term references, no reference-list reordering.

#![allow(
    clippy::redundant_pub_crate,
    reason = "workspace `unreachable_pub` policy (Cargo.toml) wants `pub(crate)` here; \
              clippy::pedantic's redundant_pub_crate disagrees for private modules — the \
              two lints are mutually exclusive for this shape, workspace policy wins"
)]

/// Fixed-capacity DPB ring size — enough for one active forward reference
/// plus in-flight pipelining headroom, not driver-dependent tuning
/// (ADR-0002's "Alternatives Considered": a bare `[Option<DpbSlot>; 4]` array
/// beats `SmallVec` here since there is no heap-spill case to avoid). The
/// crate requests `min(driver max_dpb_slots, WORKSPACE_DPB_CAP)` slots when
/// GOP encode is enabled (see `session::Capabilities::max_dpb_slots` and
/// `encoder.rs::VulkanVideoEncoder::open`).
pub(crate) const WORKSPACE_DPB_CAP: usize = 4;

/// `log2_max_frame_num_minus4` this crate's SPS uses whenever GOP encode is
/// active (`gop_size > 1` and the driver supports it) — `12` (H.264's
/// spec-legal maximum, giving `log2_max_frame_num = 16`) rather than Stage
/// 1's `0` (`log2_max_frame_num = 4`, wraps at 16). `frame_num` resets to
/// `0` at every IDR (see [`GopState::decide`]), so it only needs to stay
/// unwrapped across one GOP; picking the widest legal field sidesteps
/// implementing H.264 §8.2.4.1's `FrameNumWrap` arithmetic (needed once
/// `frame_num` itself wraps mid-GOP — irrelevant to this crate's
/// single-forward-reference design, which never needs more than the
/// immediately preceding picture) for any `gop_size` up to 65536 frames —
/// far beyond a reasonable streaming keyframe interval.
pub(crate) const LOG2_MAX_FRAME_NUM_MINUS4: u8 = 12;

/// One populated DPB slot: the `frame_num`/`PicOrderCnt`/picture-type of the
/// picture currently stored there — enough to rebuild the
/// `StdVideoEncodeH264ReferenceInfo` a later frame's read of this slot needs
/// ([`h264_params::build_reference_info`](super::h264_params::build_reference_info)).
#[derive(Debug, Clone, Copy)]
pub(crate) struct DpbSlot {
    pub(crate) frame_num: u32,
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

/// Caller-requested frame kind for [`GopState::decide`]. `Auto` is this
/// ADR's only wired case; `ForceIdr` is a hook for a future out-of-band IDR
/// request (e.g. a detected packet-loss event upstream), left unimplemented
/// by any caller this pass, per ADR-0002's design section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "ForceIdr is a documented hook, not wired by any caller yet"
)]
pub(crate) enum FrameRequest {
    Auto,
    ForceIdr,
}

/// One frame's resolved encode plan: IDR vs P, `frame_num`/`PicOrderCnt`, and
/// which DPB slot to write into / (optionally) read as the sole L0
/// reference.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FrameDecision {
    pub(crate) is_idr: bool,
    pub(crate) frame_num: u32,
    pub(crate) poc: i32,
    /// Only meaningful when `is_idr` — mirrors `VulkanVideoEncoder`'s
    /// pre-ADR-0002 `frame_counter`, byte-identical sequencing preserved
    /// (see `GopState::decide`'s doc).
    pub(crate) idr_pic_id: u16,
    pub(crate) setup_slot: usize,
    pub(crate) reference: Option<(usize, DpbSlot)>,
}

/// Per-session forward-only prediction state (ADR-0002's `GopState` sketch).
/// Owned by the caller (`VulkanVideoEncoder`), mutated in place per frame —
/// no per-frame allocation.
#[derive(Debug)]
pub(crate) struct GopState {
    gop_size: u32,
    frames_since_idr: u32,
    frame_num: u32,
    idr_counter: u16,
    dpb: Dpb,
    last_written: Option<usize>,
}

impl GopState {
    /// `gop_size == 1` reproduces Stage 1's all-IDR behavior exactly: every
    /// `decide` call returns `is_idr: true`, `frame_num: 0`, `poc: 0`,
    /// `reference: None`, and `idr_pic_id` increments `0, 1, 2, ...` — the
    /// same sequence `VulkanVideoEncoder`'s old hardcoded `frame_counter`
    /// produced, so routing every H.264 frame through this state machine
    /// (regardless of `gop_size`) keeps default output byte-identical.
    pub(crate) fn new(gop_size: u32) -> Self {
        Self {
            gop_size,
            frames_since_idr: 0,
            frame_num: 0,
            idr_counter: 0,
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
