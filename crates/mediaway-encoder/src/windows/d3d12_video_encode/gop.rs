//! Pure-Rust H.264 GOP/reference-frame decision state machine for the D3D12
//! backend — no D3D12 types, mirrors the Vulkan backend's `GopState` shape
//! (single forward reference, POC type 2: `poc = 2 * frame_num`, no B-frames).
//! See `adr/0008-d3d12-h264-gop-p-frames.md`.

/// One frame's encode decision. `is_idr` decides `FrameType`; `frame_num`/`poc`
/// are the H.264 slice-header values the driver derives its own slice header
/// from. A P frame (`!is_idr`) always references exactly the immediately
/// preceding frame — this backend's reconstructed-picture pool only ever
/// holds one live reference, so there is no separate "has a reference" flag:
/// every non-IDR frame has one by construction (the frame right before it,
/// whether that was an IDR or a P frame, was always written to the pool).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FrameDecision {
    pub(super) is_idr: bool,
    pub(super) frame_num: u32,
    pub(super) poc: u32,
}

/// Tracks `frame_num`/`PicOrderCntVal` across `push_frame` calls for one D3D12
/// H.264 GOP-mode session. `gop_size <= 1` degrades to "every frame is IDR",
/// byte-identical to this backend's pre-GOP-support behavior.
#[derive(Debug, Clone, Copy)]
pub(super) struct H264GopState {
    gop_size: u32,
    frame_index_in_gop: u32,
    frame_num: u32,
}

impl H264GopState {
    pub(super) const fn new(gop_size: u32) -> Self {
        Self {
            gop_size,
            frame_index_in_gop: 0,
            frame_num: 0,
        }
    }

    /// Advance to the next frame's decision.
    pub(super) fn decide(&mut self) -> FrameDecision {
        let is_idr = self.gop_size <= 1 || self.frame_index_in_gop == 0;
        self.frame_num = if is_idr {
            0
        } else {
            self.frame_num.wrapping_add(1)
        };
        let decision = FrameDecision {
            is_idr,
            frame_num: self.frame_num,
            poc: self.frame_num * 2,
        };
        self.frame_index_in_gop = (self.frame_index_in_gop + 1) % self.gop_size.max(1);
        decision
    }
}

#[cfg(test)]
#[path = "gop_tests.rs"]
mod tests;
