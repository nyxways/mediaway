//! GOP state for VA-API VP9 encode (`adr/linux/0004-vaapi-vp9-key-frame-and-inter-gop.md`) —
//! a small, pure-Rust 2-slot physical ping-pong state machine. Not a verbatim port (no VP9
//! GOP-state precedent exists anywhere in this workspace — see that ADR's own § "Why this ADR
//! cannot be a verbatim port"); cross-checked instead against `FFmpeg`'s real, shipping
//! `vaapi_encode_vp9.c` (`vaapi_encode_vp9_init_picture_params`'s `FF_HW_PICTURE_TYPE_P` branch,
//! quoted verbatim in that ADR).
//!
//! Deliberately narrow, matching the ADR's scope: single forward reference only (`LAST_FRAME`),
//! no `GOLDEN_FRAME`/`ALTREF_FRAME`, no B-frames (permanent non-goal), 2 physical surfaces in a
//! ping-pong (never aliasing a frame's own destination surface with its own reference surface —
//! see the ADR's § Alternatives Considered for why the ping-pong is kept even though `FFmpeg`'s own
//! `max_b_depth == 0` branch does not strictly need it).
//!
//! No `cros_libva` types or VA-API calls anywhere in this file — pure data, unit-testable
//! without any VA-API device (see `vp9_gop_tests.rs`).

#![forbid(unsafe_code)]

/// Physical ping-pong surface count — VP9's own 8 *logical* reference-frame slots never need
/// more than 2 physical buffers for this ADR's single-forward-reference scope (every P frame
/// reads exactly one prior picture as `LAST_FRAME`, either physical slot `0` or `1`).
pub(super) const WORKSPACE_PING_PONG_SLOTS: usize = 2;

/// Caller-requested frame kind for [`GopState::decide`]. `Auto` is this ADR's only wired case;
/// `ForceKey` is a hook for a future out-of-band key-frame request, left unimplemented by any
/// caller this pass — mirrors this crate's H.264 `gop::FrameRequest::ForceIdr` precedent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "ForceKey is a documented hook, not wired by any caller yet (mirrors gop::FrameRequest::ForceIdr)"
)]
pub(super) enum FrameRequest {
    Auto,
    ForceKey,
}

/// One frame's resolved encode plan: `KEY_FRAME` vs `INTER_FRAME`, which physical ping-pong slot
/// to write into, the `refresh_frame_flags` byte VA-API's `VP9EncPicFlags`/`EncPictureParameter`
/// wiring needs, and (for a P frame) which physical slot to read as the sole `LAST_FRAME`
/// reference.
#[derive(Debug, Clone, Copy)]
pub(super) struct FrameDecision {
    pub(super) is_key: bool,
    /// `0` or `1` — which physical surface this frame writes (`hpic->slot` in `FFmpeg`'s own
    /// naming).
    pub(super) setup_slot: usize,
    /// `0xff` for `KEY_FRAME`; `(1 << setup_slot) | 0xfc` for `INTER_FRAME` — the exact `FFmpeg`
    /// branch quoted in the encoder ADR (refreshes every one of VP9's 8 logical reference-frame
    /// slots to alias this frame's own physical surface, keeping every logical slot in sync with
    /// the 2-slot ping-pong; this crate's own decoder sibling's persistent 8-slot shadow table is
    /// designed to handle exactly this multi-bit aliasing pattern correctly).
    pub(super) refresh_frame_flags: u8,
    /// Physical slot index of this `INTER_FRAME`'s sole `LAST_FRAME` reference — the ping-pong
    /// slot *not* being refreshed this frame. `None` on every `KEY_FRAME`.
    pub(super) reference_slot: Option<usize>,
}

/// Per-session VP9 ping-pong GOP state (the encoder ADR's own `GopState` sketch). Owned by the
/// caller (`VaapiVp9Encoder`), mutated in place per frame — no per-frame allocation.
#[derive(Debug)]
pub(super) struct GopState {
    gop_size: u32,
    frames_since_key: u32,
    /// Physical slot (`0` or `1`) written by the most recently decided frame — `None` only
    /// before the first `decide()` call.
    last_written: Option<usize>,
}

impl GopState {
    /// `gop_size == 1` reproduces this crate's `KEY_FRAME`-only baseline exactly: every
    /// `decide()` call returns `is_key: true`, `setup_slot: 0`, `refresh_frame_flags: 0xff`,
    /// `reference_slot: None` — so routing every VP9 frame through this state machine
    /// (regardless of `gop_size`) keeps the default output byte-identical.
    pub(super) const fn new(gop_size: u32) -> Self {
        Self {
            gop_size,
            frames_since_key: 0,
            last_written: None,
        }
    }

    pub(super) fn decide(&mut self, request: FrameRequest) -> FrameDecision {
        let is_key = matches!(request, FrameRequest::ForceKey)
            || self.frames_since_key == 0
            || self.frames_since_key >= self.gop_size;
        if is_key {
            self.frames_since_key = 0;
        }

        let (setup_slot, refresh_frame_flags, reference_slot) = if is_key {
            // `FFmpeg`'s own IDR branch: `hpic->slot = 0`, `refresh_frame_flags = 0xff`.
            (0usize, 0xffu8, None)
        } else {
            // `FFmpeg`'s own ping-pong P branch: `hpic->slot = !href->slot`,
            // `refresh_frame_flags = 1 << hpic->slot | 0xfc`.
            let prev = self.last_written.unwrap_or(0);
            let setup_slot = (prev + 1) % WORKSPACE_PING_PONG_SLOTS;
            let refresh_frame_flags = (1u8 << setup_slot) | 0xfc;
            (setup_slot, refresh_frame_flags, Some(prev))
        };

        self.last_written = Some(setup_slot);
        self.frames_since_key += 1;

        FrameDecision {
            is_key,
            setup_slot,
            refresh_frame_flags,
            reference_slot,
        }
    }
}

#[cfg(test)]
#[path = "vp9_gop_tests.rs"]
mod tests;
