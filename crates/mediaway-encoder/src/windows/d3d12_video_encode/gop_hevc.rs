//! Pure-Rust HEVC GOP/reference-frame decision state machine for the D3D12
//! backend — mirrors [`super::gop`]'s H.264 `H264GopState`, but simpler: HEVC
//! has no `frame_num`/`idr_pic_id` concept, only a `PictureOrderCountNumber`
//! that increments by one per frame and resets at every IDR. Same
//! single-forward-reference, no-B-frames scope as the H.264 side — see
//! `adr/windows/0007-d3d12-native-video-encode.md`'s 2026-08-06 addendum.

/// One frame's encode decision. `is_idr` decides `FrameType`; `poc` is the
/// HEVC `PictureOrderCountNumber` the driver derives its own slice header
/// from. A P frame (`!is_idr`) always references exactly the immediately
/// preceding frame — see [`super::gop::FrameDecision`]'s doc for why no
/// separate "has a reference" flag is needed.
///
/// `intra_refresh_frame_index` is `Some(i)` (`i` in `[0, period)`) on every
/// frame of an intra-refresh session (see [`HevcGopState::new_intra_refresh`]);
/// `None` outside that mode, and on that mode's own startup IDR frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FrameDecision {
    pub(super) is_idr: bool,
    pub(super) poc: u32,
    pub(super) intra_refresh_frame_index: Option<u32>,
}

/// Tracks `PictureOrderCountNumber` (and, in intra-refresh mode, the current
/// refresh-wave index) across `push_frame` calls for one D3D12 HEVC session.
/// `gop_size <= 1` (via [`Self::new`]) degrades to "every frame is IDR",
/// byte-identical to this backend's pre-GOP-support behavior.
#[derive(Debug, Clone, Copy)]
pub(super) struct HevcGopState {
    gop_size: u32,
    /// `0` = intra refresh disabled; `> 0` = wave length in frames, set only
    /// via [`Self::new_intra_refresh`] — see [`super::gop::H264GopState`]'s
    /// sibling field doc.
    intra_refresh_period: u32,
    started: bool,
    frame_index_in_gop: u32,
    poc: u32,
    intra_refresh_frame_index: u32,
}

impl HevcGopState {
    /// Periodic-GOP (or, at `gop_size <= 1`, IDR-only) mode.
    pub(super) const fn new(gop_size: u32) -> Self {
        Self {
            gop_size,
            intra_refresh_period: 0,
            started: false,
            frame_index_in_gop: 0,
            poc: 0,
            intra_refresh_frame_index: 0,
        }
    }

    /// Intra-refresh mode — see [`super::gop::H264GopState::new_intra_refresh`].
    pub(super) const fn new_intra_refresh(period: u32) -> Self {
        Self {
            gop_size: 0,
            intra_refresh_period: period,
            started: false,
            frame_index_in_gop: 0,
            poc: 0,
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
        self.poc = if is_idr { 0 } else { self.poc.wrapping_add(1) };
        let intra_refresh_frame_index = (self.intra_refresh_period > 0 && !is_idr).then(|| {
            let index = self.intra_refresh_frame_index;
            self.intra_refresh_frame_index = (index + 1) % self.intra_refresh_period;
            index
        });
        let decision = FrameDecision {
            is_idr,
            poc: self.poc,
            intra_refresh_frame_index,
        };
        self.frame_index_in_gop = (self.frame_index_in_gop + 1) % self.gop_size.max(1);
        decision
    }
}

#[cfg(test)]
#[path = "gop_hevc_tests.rs"]
mod tests;
