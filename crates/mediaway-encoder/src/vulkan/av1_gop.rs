//! Multi-frame GOP state for AV1 `INTER_FRAME` encode (ADR-0002's AV1
//! follow-up) — AV1 sibling of [`super::h264_gop`]/[`super::hevc_gop`]. Same
//! pure-Rust, no-FFI, no-`unsafe` DPB ring buffer and single-forward-reference
//! `decide` state machine; a separate type from both siblings (same reasoning
//! `hevc_gop`'s module doc already gives for not sharing with `h264_gop`) —
//! AV1's reference model is `order_hint`-keyed (`StdVideoEncodeAV1PictureInfo`
//! has no `FrameNum`/`PicOrderCnt` at all), and its `DpbSlot` additionally
//! needs `is_key` (vs. `is_idr`) to match AV1's own `StdVideoAV1FrameType`
//! naming.
//!
//! **This crate's AV1 base (IDR-only) encode is already hardware-verified
//! *not* to produce a valid per-frame OBU on this crate's reference RTX 4090
//! — a driver-maturity limitation, not a bug in this crate's own bitstream
//! construction (see `adr/0001`'s AV1 addendum and `adr/vulkan/0002`'s AV1
//! follow-up section). This module is real, capability-gated GOP wiring built
//! on top of that known-broken base — implemented so the shape exists and the
//! capability gate is real, but genuinely unverifiable on this hardware. See
//! `encoder_tests.rs::push_seven_av1_frames_gop_or_skip`, which honestly
//! skips rather than asserting a result nobody can currently observe.**
//!
//! AV1's reference model is structurally wider than H.264/HEVC's (up to
//! [`vulkanalia::vk::video::STD_VIDEO_AV1_REFS_PER_FRAME`] = 7 named
//! reference slots, [`vulkanalia::vk::video::STD_VIDEO_AV1_NUM_REF_FRAMES`] =
//! 8 physical DPB slots) — this crate keeps the same single-forward-reference
//! scope as H.264/HEVC (`LAST_FRAME` only, never `LAST2`/`LAST3`/`GOLDEN`/
//! `BWDREF`/`ALTREF2`/`ALTREF`), matching ADR-0002's narrow design. One
//! physical [`super::h264_gop::WORKSPACE_DPB_CAP`] ring slot doubles as the
//! AV1-bitstream-level reference-frame-slot number this frame's
//! `refresh_frame_flags` bit and `ref_frame_idx[LAST_FRAME]` both address —
//! see [`super::av1_params::InterFramePrediction`]'s doc for how a
//! [`FrameDecision`] becomes the `StdVideoEncodeAV1*` fields that model
//! requires.

#![allow(
    clippy::redundant_pub_crate,
    reason = "workspace `unreachable_pub` policy (Cargo.toml) wants `pub(crate)` here; \
              clippy::pedantic's redundant_pub_crate disagrees for private modules — the \
              two lints are mutually exclusive for this shape, workspace policy wins"
)]

use super::h264_gop::WORKSPACE_DPB_CAP;

/// `order_hint_bits_minus_1` this crate's sequence header uses whenever GOP
/// encode is active — `7` (AV1 spec's legal maximum, `OrderHintBits = 8`,
/// `order_hint` wraps mod 256) rather than the base path's `6`
/// (`OrderHintBits = 7`, wraps mod 128, [`av1_params::build_sequence_header`](super::av1_params::build_sequence_header)'s
/// original hardcoded value, kept unchanged by default). `order_hint` resets
/// to `0` at every key frame (see [`GopState::decide`]), so — same reasoning
/// as [`h264_gop::LOG2_MAX_FRAME_NUM_MINUS4`](super::h264_gop::LOG2_MAX_FRAME_NUM_MINUS4)/
/// [`hevc_gop::LOG2_MAX_PIC_ORDER_CNT_LSB_MINUS4`](super::hevc_gop::LOG2_MAX_PIC_ORDER_CNT_LSB_MINUS4)
/// — picking the widest legal field sidesteps wraparound arithmetic for any
/// `gop_size` up to 256 frames. **Narrower headroom than H.264/HEVC's
/// 65536-frame ceiling**: AV1's own spec caps `order_hint_bits_minus_1` at
/// `7` (8 bits is the field's maximum width), an inherent format limit this
/// crate cannot widen further — a real deviation from the other two codecs'
/// "any practical GOP size" guarantee, not an oversight.
pub(crate) const ORDER_HINT_BITS_MINUS_1_GOP: u8 = 7;

/// One populated DPB slot: the `order_hint`/frame-type of the picture
/// currently stored there — enough to rebuild the
/// `StdVideoEncodeAV1ReferenceInfo` a later frame's read of this slot needs
/// ([`av1_params::build_reference_info`](super::av1_params::build_reference_info)).
#[derive(Debug, Clone, Copy)]
pub(crate) struct DpbSlot {
    pub(crate) order_hint: u8,
    pub(crate) is_key: bool,
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
/// [`h264_gop::FrameRequest`](super::h264_gop::FrameRequest)/
/// [`hevc_gop::FrameRequest`](super::hevc_gop::FrameRequest); duplicated
/// rather than shared since the three `GopState`s are otherwise independent
/// types (see this module's doc for why).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "ForceKey is a documented hook, not wired by any caller yet"
)]
pub(crate) enum FrameRequest {
    Auto,
    ForceKey,
}

/// One frame's resolved encode plan: key vs. inter, `order_hint`, and which
/// DPB slot to write into / (optionally) read as the sole `LAST_FRAME`
/// reference.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FrameDecision {
    pub(crate) is_key: bool,
    pub(crate) order_hint: u8,
    pub(crate) setup_slot: usize,
    pub(crate) reference: Option<(usize, DpbSlot)>,
}

/// Per-session forward-only prediction state — AV1 sibling of
/// [`h264_gop::GopState`](super::h264_gop::GopState)/
/// [`hevc_gop::GopState`](super::hevc_gop::GopState). Owned by the caller
/// (`VulkanVideoEncoder`), mutated in place per frame — no per-frame
/// allocation.
#[derive(Debug)]
pub(crate) struct GopState {
    gop_size: u32,
    frames_since_key: u32,
    order_hint: u8,
    dpb: Dpb,
    last_written: Option<usize>,
}

impl GopState {
    /// `gop_size == 1` reproduces the base path's all-key-frame behavior
    /// exactly: every `decide` call returns `is_key: true, order_hint: 0,
    /// reference: None` — the same values
    /// [`super::av1_params::build_key_frame_picture_info`] already hardcodes,
    /// so routing every AV1 frame through this state machine (regardless of
    /// `gop_size`) keeps the default path's per-frame construction
    /// byte-identical.
    pub(crate) fn new(gop_size: u32) -> Self {
        Self {
            gop_size,
            frames_since_key: 0,
            order_hint: 0,
            dpb: Dpb::default(),
            last_written: None,
        }
    }

    pub(crate) fn decide(&mut self, request: FrameRequest) -> FrameDecision {
        let is_key = matches!(request, FrameRequest::ForceKey)
            || self.frames_since_key == 0
            || self.frames_since_key >= self.gop_size;
        if is_key {
            // A key frame refreshes every one of AV1's 8 reference-frame
            // slots per spec (`refresh_frame_flags = 0xFF`) — this crate's
            // DPB mirrors that by discarding all tracked slot state and
            // restarting the ring.
            self.order_hint = 0;
            self.frames_since_key = 0;
            self.dpb = Dpb::default();
            self.last_written = None;
        }

        let setup_slot = self.dpb.next_slot;
        let reference = if is_key {
            None
        } else {
            self.last_written
                .and_then(|slot| self.dpb.slots[slot].map(|dpb_slot| (slot, dpb_slot)))
        };
        let order_hint = self.order_hint;

        let decision = FrameDecision {
            is_key,
            order_hint,
            setup_slot,
            reference,
        };

        // Bookkeeping for the frame just decided: record it into its own
        // setup slot so a *future* `decide` call can read it back as
        // `reference`, then advance every counter for the *next* call.
        self.dpb.slots[setup_slot] = Some(DpbSlot { order_hint, is_key });
        self.dpb.next_slot = (setup_slot + 1) % WORKSPACE_DPB_CAP;
        self.last_written = Some(setup_slot);
        // `order_hint` is a `u8` field on the std struct and this crate's
        // GOP mode widens `order_hint_bits_minus_1` to `7` (`OrderHintBits =
        // 8`) whenever GOP is active — a plain `u8` wrap is exactly AV1's
        // spec-legal mod-256 `order_hint` arithmetic for that width. The
        // default (`gop_size == 1`) path never advances past `0` (every call
        // is a key frame, which resets it first), so this wrap is never
        // reached there.
        self.order_hint = self.order_hint.wrapping_add(1);
        self.frames_since_key += 1;

        decision
    }
}
