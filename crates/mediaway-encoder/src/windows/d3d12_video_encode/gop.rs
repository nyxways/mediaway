//! Pure-Rust H.264 GOP/reference-frame decision state machine for the D3D12
//! backend — no D3D12 types, mirrors the Vulkan backend's `GopState` shape
//! (single forward reference, POC type 2: `poc = 2 * frame_num`, no B-frames).
//! See `adr/windows/0007-d3d12-native-video-encode.md`'s 2026-08-06 addendum.

/// One frame's encode decision. `is_idr` decides `FrameType`; `frame_num`/`poc`
/// are the H.264 slice-header values the driver derives its own slice header
/// from. A P frame (`!is_idr`) always references exactly the immediately
/// preceding frame — this backend's reconstructed-picture pool only ever
/// holds one live reference, so there is no separate "has a reference" flag:
/// every non-IDR frame has one by construction (the frame right before it,
/// whether that was an IDR or a P frame, was always written to the pool).
///
/// `intra_refresh_frame_index` is `Some(i)` (`i` in `[0, period)`) on every
/// frame of an intra-refresh session (see [`H264GopState::new_intra_refresh`]);
/// `None` outside that mode, and on that mode's own startup IDR frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FrameDecision {
    pub(super) is_idr: bool,
    pub(super) frame_num: u32,
    pub(super) poc: u32,
    pub(super) intra_refresh_frame_index: Option<u32>,
}

/// Tracks `frame_num`/`PicOrderCntVal` (and, in intra-refresh mode, the
/// current refresh-wave index) across `push_frame` calls for one D3D12 H.264
/// session. `gop_size <= 1` (via [`Self::new`]) degrades to "every frame is
/// IDR", byte-identical to this backend's pre-GOP-support behavior.
#[derive(Debug, Clone, Copy)]
pub(super) struct H264GopState {
    /// Periodic-IDR cadence; unused (see `intra_refresh_period`) once intra
    /// refresh is active — that mode requires an unbounded GOP instead.
    gop_size: u32,
    /// `0` = intra refresh disabled (plain periodic-GOP or IDR-only mode,
    /// per [`Self::new`]). `> 0` = intra-refresh wave length in frames, set
    /// only via [`Self::new_intra_refresh`].
    intra_refresh_period: u32,
    /// Whether the very first frame this state machine will ever decide has
    /// already been produced — only meaningful in intra-refresh mode, where
    /// it is the sole trigger for `is_idr` (that mode's GOP never re-IDRs).
    started: bool,
    frame_index_in_gop: u32,
    frame_num: u32,
    intra_refresh_frame_index: u32,
}

impl H264GopState {
    /// Periodic-GOP (or, at `gop_size <= 1`, IDR-only) mode.
    pub(super) const fn new(gop_size: u32) -> Self {
        Self {
            gop_size,
            intra_refresh_period: 0,
            started: false,
            frame_index_in_gop: 0,
            frame_num: 0,
            intra_refresh_frame_index: 0,
        }
    }

    /// Intra-refresh mode: an unbounded GOP (the session's only IDR is its
    /// first frame) with continuous, back-to-back row-based refresh waves of
    /// `period` frames each.
    pub(super) const fn new_intra_refresh(period: u32) -> Self {
        Self {
            gop_size: 0,
            intra_refresh_period: period,
            started: false,
            frame_index_in_gop: 0,
            frame_num: 0,
            intra_refresh_frame_index: 0,
        }
    }

    /// Advance to the next frame's decision.
    pub(super) fn decide(&mut self) -> FrameDecision {
        let is_idr = if self.intra_refresh_period > 0 {
            !self.started
        } else {
            self.gop_size <= 1 || self.frame_index_in_gop == 0
        };
        self.started = true;
        self.frame_num = if is_idr {
            0
        } else {
            self.frame_num.wrapping_add(1)
        };
        let intra_refresh_frame_index = (self.intra_refresh_period > 0 && !is_idr).then(|| {
            let index = self.intra_refresh_frame_index;
            self.intra_refresh_frame_index = (index + 1) % self.intra_refresh_period;
            index
        });
        let decision = FrameDecision {
            is_idr,
            frame_num: self.frame_num,
            poc: self.frame_num * 2,
            intra_refresh_frame_index,
        };
        self.frame_index_in_gop = (self.frame_index_in_gop + 1) % self.gop_size.max(1);
        decision
    }
}

#[cfg(test)]
#[path = "gop_tests.rs"]
mod tests;
