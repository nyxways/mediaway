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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FrameDecision {
    pub(super) is_idr: bool,
    pub(super) poc: u32,
}

/// Tracks `PictureOrderCountNumber` across `push_frame` calls for one D3D12
/// HEVC GOP-mode session. `gop_size <= 1` degrades to "every frame is IDR",
/// byte-identical to this backend's pre-GOP-support behavior.
#[derive(Debug, Clone, Copy)]
pub(super) struct HevcGopState {
    gop_size: u32,
    frame_index_in_gop: u32,
    poc: u32,
}

impl HevcGopState {
    pub(super) const fn new(gop_size: u32) -> Self {
        Self {
            gop_size,
            frame_index_in_gop: 0,
            poc: 0,
        }
    }

    /// Advance to the next frame's decision.
    pub(super) fn decide(&mut self) -> FrameDecision {
        let is_idr = self.gop_size <= 1 || self.frame_index_in_gop == 0;
        self.poc = if is_idr { 0 } else { self.poc.wrapping_add(1) };
        let decision = FrameDecision {
            is_idr,
            poc: self.poc,
        };
        self.frame_index_in_gop = (self.frame_index_in_gop + 1) % self.gop_size.max(1);
        decision
    }
}

#[cfg(test)]
#[path = "gop_hevc_tests.rs"]
mod tests;
