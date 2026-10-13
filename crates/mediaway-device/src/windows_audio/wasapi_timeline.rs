//! The capture loop's sample timeline: which silence to insert and which captured frames to
//! drop so that a WASAPI loopback stream's sample count follows the wall clock.
//!
//! Pure bookkeeping — no COM, no clock reads. `pump_capture_loop` (`wasapi.rs`) feeds it the
//! QPC times WASAPI reports and the QPC "now" it reads itself; everything here is driven by
//! those numbers, so `wasapi_timeline_tests.rs` covers it without audio hardware.
//!
//! # Why
//!
//! A capture's frame `pts` is its sample count. That only tracks wall time if a sample is
//! delivered for every sample period, and WASAPI loopback breaks that two ways (measured on
//! Windows 11 26300, ADR-0002 § Correction (2026-10-13)):
//!
//! - an active endpoint playing silence delivers `AUDCLNT_BUFFERFLAGS_SILENT` packets (handled in
//!   `wasapi.rs`: delivered as zeros, not dropped);
//! - an **idle** render endpoint (no render stream open) delivers **no packets at all** — the
//!   first packet arrives when something starts playing, carrying a QPC time seconds after the
//!   last one. Its device position restarts too, so only the QPC time records the gap.
//!
//! This timeline fills the second case with silence: live while the endpoint stays idle
//! ([`SampleTimeline::idle`]), and for any gap a packet's QPC time reveals
//! ([`SampleTimeline::packet`]).
//!
//! Prior art (ideas only, no code): OBS `plugins/win-wasapi/win-wasapi.cpp` (GPL-2.0-or-later)
//! keeps loopback flowing with a silent render stream and stamps loopback audio with the
//! `GetBuffer` QPC time by default; Chromium `media/audio/win/audio_low_latency_input_win.cc`
//! (BSD-3-Clause) pushes silence for `AUDCLNT_BUFFERFLAGS_SILENT` and drops QPC times more than
//! 500 ms off the wall clock (crbug.com/1439283) — [`MAX_CLOCK_SKEW_HNS`] is that rule.

#![allow(
    clippy::redundant_pub_crate,
    reason = "private module; `pub(crate)` states the real reach (same as `wasapi_process.rs`)"
)]

/// 100-ns ticks per second — the unit of `IAudioCaptureClient::GetBuffer`'s QPC position.
const HNS_PER_SEC: u64 = 10_000_000;

/// A packet that starts later than the last one ended by more than this is preceded by
/// silence for the difference. Consecutive packets measured 0.0 ms apart on every source
/// tried; the one real discontinuity seen (a render stream opening during process loopback)
/// was 12.2 ms. Smaller than one 10 ms period, so a dropped period is always filled.
pub(crate) const GAP_TOLERANCE_HNS: u64 = 40_000;

/// While no packet arrives, silence is filled up to this far behind "now", never closer: a
/// packet's QPC time was measured up to 15 ms in the past (process loopback) and up to 30 ms
/// in the future (endpoint loopback) of the moment it was read, so a real packet never lands
/// inside silence already filled. The cost: an idle stream's samples lag the wall clock by
/// this much (a consumer that cuts the stream by wall time, e.g. a pause, is off by up to
/// 100 ms of silence).
pub(crate) const IDLE_LAG_HNS: u64 = 1_000_000;

/// Idle silence is handed out in pieces of at least this length (20 ms), not one per poll.
pub(crate) const IDLE_MIN_FILL_HNS: u64 = 200_000;

/// At most this much idle silence (1 s) per [`SampleTimeline::idle`] call, so catching up
/// after a long stall cannot burst past the capture queue's capacity in one go.
pub(crate) const IDLE_MAX_FILL_HNS: u64 = HNS_PER_SEC;

/// A packet whose QPC time is further than this (500 ms) from the QPC read just after it
/// makes the session stop trusting device timestamps for good: from then on frames are only
/// counted, as before this timeline existed. Chromium's rule for the same failure.
pub(crate) const MAX_CLOCK_SKEW_HNS: u64 = 5_000_000;

/// What to do around one captured packet.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PacketPlan {
    /// Silent sample frames to deliver before the packet.
    pub fill: u64,
    /// Sample frames to drop from the packet's head: already covered by idle silence.
    pub skip: u32,
}

/// Where the next delivered sample frame belongs on the QPC clock.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SampleTimeline {
    rate: u64,
    /// `false`: frames are only counted (microphones, or QPC times proved unusable).
    wall_clock: bool,
    /// QPC time (100 ns) of the frame at `since_anchor == 0`.
    anchor_hns: u64,
    /// Frames delivered since `anchor_hns`.
    since_anchor: u64,
    /// The last frames delivered were idle silence, so a packet that overlaps them is the
    /// real audio for time already filled, and its overlapping head is dropped.
    synthetic: bool,
}

impl SampleTimeline {
    /// `start_hns`: the QPC time the stream started (frame 0). `wall_clock`: fill gaps from QPC
    /// times (loopback); without it, or without a start time, frames are only counted.
    pub(crate) fn new(sample_rate: u32, start_hns: Option<u64>, wall_clock: bool) -> Self {
        Self {
            rate: u64::from(sample_rate.max(1)),
            wall_clock: wall_clock && start_hns.is_some(),
            anchor_hns: start_hns.unwrap_or(0),
            since_anchor: 0,
            synthetic: false,
        }
    }

    /// Whether gaps are being filled from QPC times.
    pub(crate) const fn follows_wall_clock(&self) -> bool {
        self.wall_clock
    }

    /// A packet of `frames` frames whose first frame WASAPI stamped `qpc_hns` (`None`:
    /// no usable stamp, e.g. `AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR`), read at `now_hns`.
    pub(crate) fn packet(
        &mut self,
        frames: u32,
        qpc_hns: Option<u64>,
        now_hns: Option<u64>,
    ) -> PacketPlan {
        if let (Some(qpc), Some(now)) = (qpc_hns, now_hns)
            && qpc.abs_diff(now) > MAX_CLOCK_SKEW_HNS
        {
            self.wall_clock = false;
        }
        let Some(qpc) = qpc_hns.filter(|_| self.wall_clock) else {
            self.since_anchor += u64::from(frames);
            self.synthetic = false;
            return PacketPlan::default();
        };
        let next = self.next_hns();
        let mut plan = PacketPlan::default();
        if qpc > next.saturating_add(GAP_TOLERANCE_HNS) {
            plan.fill = self.frames_in(qpc - next, true);
        } else if qpc < next && self.synthetic {
            let covered = self.frames_in(next - qpc, true);
            plan.skip = u32::try_from(covered).unwrap_or(u32::MAX).min(frames);
            if plan.skip == frames {
                // Entirely inside silence already delivered: nothing of it is new.
                return plan;
            }
        }
        // Re-anchor on the packet's own stamp, so the device clock's drift against QPC never
        // accumulates into a gap (sub-tolerance jitter is absorbed here, not filled).
        self.anchor_hns = qpc;
        self.since_anchor = u64::from(frames);
        self.synthetic = false;
        plan
    }

    /// No packet is waiting at `now_hns`: silent frames to deliver now (0 most of the time).
    pub(crate) fn idle(&mut self, now_hns: u64) -> u64 {
        if !self.wall_clock {
            return 0;
        }
        let next = self.next_hns();
        let target = now_hns.saturating_sub(IDLE_LAG_HNS);
        if target < next.saturating_add(IDLE_MIN_FILL_HNS) {
            return 0;
        }
        let frames = self.frames_in((target - next).min(IDLE_MAX_FILL_HNS), false);
        self.since_anchor += frames;
        self.synthetic = true;
        frames
    }

    /// QPC time (100 ns) the next delivered frame belongs at.
    fn next_hns(&self) -> u64 {
        let offset =
            u128::from(self.since_anchor) * u128::from(HNS_PER_SEC) / u128::from(self.rate);
        self.anchor_hns
            .saturating_add(u64::try_from(offset).unwrap_or(u64::MAX))
    }

    /// Sample frames in `hns` 100-ns ticks, rounded to nearest or down.
    fn frames_in(&self, hns: u64, nearest: bool) -> u64 {
        let half = if nearest {
            u128::from(HNS_PER_SEC) / 2
        } else {
            0
        };
        let frames = (u128::from(hns) * u128::from(self.rate) + half) / u128::from(HNS_PER_SEC);
        u64::try_from(frames).unwrap_or(u64::MAX)
    }
}

#[cfg(test)]
#[path = "wasapi_timeline_tests.rs"]
mod tests;
