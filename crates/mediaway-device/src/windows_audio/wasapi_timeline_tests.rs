#![allow(clippy::unwrap_used, clippy::expect_used, reason = "unit tests")]

use super::{IDLE_LAG_HNS, IDLE_MAX_FILL_HNS, MAX_CLOCK_SKEW_HNS, PacketPlan, SampleTimeline};

const RATE: u32 = 48_000;
/// One 10 ms WASAPI period at 48 kHz.
const PERIOD: u32 = 480;
const PERIOD_HNS: u64 = 100_000;
const MS: u64 = 10_000;
// The worst jitter measured (1.7 ms) is inside the tolerance.
const _: () = assert!(super::GAP_TOLERANCE_HNS > 17 * MS / 10);
/// An arbitrary QPC origin, so nothing depends on QPC starting at 0.
const T0: u64 = 7_000_000_000;

fn loopback() -> SampleTimeline {
    SampleTimeline::new(RATE, Some(T0), true)
}

/// Delivered frames (fill + kept packet frames) for a run of packets, polling `idle` every
/// 5 ms between them the way `pump_capture_loop` does.
struct Run {
    timeline: SampleTimeline,
    delivered: u64,
    now: u64,
}

impl Run {
    fn new(timeline: SampleTimeline) -> Self {
        Self {
            timeline,
            delivered: 0,
            now: T0,
        }
    }

    /// Advance the clock to `until`, with no packet waiting.
    fn idle_until(&mut self, until: u64) {
        while self.now < until {
            self.now = (self.now + 5 * MS).min(until);
            self.delivered += self.timeline.idle(self.now);
        }
    }

    /// A packet stamped `qpc`, read `read_lag` after it (may be negative: endpoint loopback
    /// stamps are in the future).
    fn packet(&mut self, qpc: u64, read_at: u64) -> PacketPlan {
        self.now = self.now.max(read_at);
        let plan = self.timeline.packet(PERIOD, Some(qpc), Some(self.now));
        self.delivered += plan.fill + u64::from(PERIOD - plan.skip);
        plan
    }

    /// Frames that wall time from `T0` to `at` is worth.
    fn wall_frames(at: u64) -> u64 {
        (at - T0) * u64::from(RATE) / 10_000_000
    }
}

#[test]
fn back_to_back_packets_need_nothing() {
    let mut t = loopback();
    for i in 0..100 {
        assert_eq!(
            t.packet(PERIOD, Some(T0 + i * PERIOD_HNS), Some(T0 + i * PERIOD_HNS)),
            PacketPlan::default()
        );
    }
}

/// Sub-tolerance jitter, either way, is absorbed — never turned into inserted silence or a
/// trimmed packet, which would be an audible glitch in continuous sound.
#[test]
fn jitter_within_tolerance_is_absorbed() {
    let mut t = loopback();
    let mut qpc = T0;
    for i in 0..200u64 {
        let jitter = if i % 2 == 0 { 17 * MS / 10 } else { 0 }; // 1.7 ms, the worst measured
        let plan = t.packet(PERIOD, Some(qpc + jitter), Some(qpc));
        assert_eq!(plan, PacketPlan::default(), "packet {i}");
        qpc += PERIOD_HNS;
    }
}

/// The measured idle-endpoint case: nothing for 1.5 s, then sound. Sample count must match
/// wall time when the sound starts, and the sound's first frame must sit at its QPC time.
#[test]
fn idle_endpoint_then_sound_keeps_samples_on_wall_time() {
    let mut run = Run::new(loopback());
    let first = T0 + 1_517 * MS;
    run.idle_until(first - 6 * MS);
    // Live idle fill has kept the count within IDLE_LAG (+ one piece) of the wall clock.
    let behind = Run::wall_frames(run.now) - run.delivered;
    assert!(
        behind <= (IDLE_LAG_HNS + 20 * MS + 5 * MS) * u64::from(RATE) / 10_000_000,
        "idle fill lags by {behind} frames"
    );
    let before = run.delivered;
    let plan = run.packet(first, first - 6 * MS);
    assert_eq!(plan.skip, 0, "nothing real was dropped");
    // The sound's first frame lands exactly where its QPC time says.
    assert_eq!(before + plan.fill, Run::wall_frames(first));
}

/// Without live idle fill (e.g. the loop was starved), the gap is still filled from the next
/// packet's QPC time.
#[test]
fn a_gap_is_filled_from_the_packet_stamp() {
    let mut t = loopback();
    assert_eq!(t.packet(PERIOD, Some(T0), Some(T0)), PacketPlan::default());
    // The next packet is 12.2 ms late — the discontinuity measured on process loopback.
    let late = T0 + PERIOD_HNS + 122 * MS / 10;
    let plan = t.packet(PERIOD, Some(late), Some(late));
    assert_eq!(plan.fill, 586); // 12.2 ms at 48 kHz, rounded
    assert_eq!(plan.skip, 0);
}

/// A stream that starts silent (nothing ever played) gets its leading silence too: frame 0
/// is the stream's start, not the first packet.
#[test]
fn leading_silence_before_the_first_packet() {
    let mut t = loopback();
    let first = T0 + 2 * 10_000_000;
    let plan = t.packet(PERIOD, Some(first), Some(first));
    assert_eq!(plan.fill, 2 * u64::from(RATE));
}

/// A real packet that overlaps silence already filled loses its overlapping head — the
/// timeline never runs ahead of the wall clock.
#[test]
fn a_packet_overlapping_filled_silence_is_trimmed() {
    let mut t = loopback();
    let filled = t.idle(T0 + IDLE_LAG_HNS + 500 * MS);
    assert_eq!(filled, 24_000); // 500 ms
    // Stamped 4 ms before the end of the filled silence.
    let plan = t.packet(PERIOD, Some(T0 + 496 * MS), Some(T0 + 500 * MS));
    assert_eq!(plan, PacketPlan { fill: 0, skip: 192 });
    // Next packet continues normally.
    assert_eq!(
        t.packet(PERIOD, Some(T0 + 506 * MS), Some(T0 + 506 * MS)),
        PacketPlan::default()
    );
}

#[test]
fn a_packet_wholly_inside_filled_silence_is_dropped() {
    let mut t = loopback();
    let _ = t.idle(T0 + IDLE_LAG_HNS + 500 * MS);
    let plan = t.packet(PERIOD, Some(T0 + 100 * MS), Some(T0 + 500 * MS));
    assert_eq!(plan.skip, PERIOD);
    // Still on the filled timeline: a packet right after the fill is kept whole.
    assert_eq!(
        t.packet(PERIOD, Some(T0 + 500 * MS), Some(T0 + 500 * MS)),
        PacketPlan::default()
    );
}

/// Outside of filled silence a slightly early packet is never trimmed (no real sound lost).
#[test]
fn early_packets_after_real_ones_are_not_trimmed() {
    let mut t = loopback();
    let _ = t.packet(PERIOD, Some(T0), Some(T0));
    let plan = t.packet(PERIOD, Some(T0 + 5 * MS), Some(T0 + 5 * MS));
    assert_eq!(plan, PacketPlan::default());
}

/// Idle fill is spread over polls and bounded per call.
#[test]
fn idle_fill_waits_for_the_lag_and_is_bounded() {
    let mut t = loopback();
    assert_eq!(t.idle(T0 + IDLE_LAG_HNS), 0);
    assert_eq!(
        t.idle(T0 + IDLE_LAG_HNS + 10 * MS),
        0,
        "below the minimum piece"
    );
    assert_eq!(t.idle(T0 + IDLE_LAG_HNS + 20 * MS), 960);
    let mut long = loopback();
    let first = long.idle(T0 + 60 * 10_000_000);
    assert_eq!(first, IDLE_MAX_FILL_HNS * u64::from(RATE) / 10_000_000);
}

/// An hour of idle polling every 5 ms delivers exactly the wall time (minus the lag): the
/// fill is computed from one anchor, so per-call rounding cannot accumulate.
#[test]
fn an_hour_of_idle_fill_does_not_drift() {
    let mut run = Run::new(SampleTimeline::new(44_100, Some(T0), true));
    let hour = T0 + 3_600 * 10_000_000;
    let mut delivered = 0u64;
    let mut now = T0;
    while now < hour {
        now += 5 * MS;
        delivered += run.timeline.idle(now);
    }
    let want = (hour - IDLE_LAG_HNS - T0) * 44_100 / 10_000_000;
    assert!(
        want - delivered <= 44_100 * 20 / 1000,
        "{want} vs {delivered}"
    );
}

/// Microphones (`wall_clock == false`) are only counted, as before.
#[test]
fn counting_only_never_fills_or_trims() {
    let mut t = SampleTimeline::new(RATE, Some(T0), false);
    assert!(!t.follows_wall_clock());
    assert_eq!(t.idle(T0 + 10 * 10_000_000), 0);
    let late = T0 + 5 * 10_000_000;
    assert_eq!(
        t.packet(PERIOD, Some(late), Some(late)),
        PacketPlan::default()
    );
    assert!(!SampleTimeline::new(RATE, None, true).follows_wall_clock());
}

/// A stamp more than 500 ms off the wall clock turns gap filling off for the session
/// (Chromium's crbug.com/1439283 rule) instead of inserting seconds of bogus silence.
#[test]
fn a_skewed_stamp_disables_wall_clock_filling() {
    let mut t = loopback();
    let now = T0 + 20 * MS;
    let skewed = now + MAX_CLOCK_SKEW_HNS + MS;
    assert_eq!(
        t.packet(PERIOD, Some(skewed), Some(now)),
        PacketPlan::default()
    );
    assert!(!t.follows_wall_clock());
    assert_eq!(t.idle(T0 + 10 * 10_000_000), 0);
}

/// A packet without a usable stamp is counted, and the next stamped packet re-anchors.
#[test]
fn a_packet_without_stamp_is_counted() {
    let mut t = loopback();
    assert_eq!(t.packet(PERIOD, Some(T0), Some(T0)), PacketPlan::default());
    assert_eq!(t.packet(PERIOD, None, None), PacketPlan::default());
    let third = T0 + 2 * PERIOD_HNS;
    assert_eq!(
        t.packet(PERIOD, Some(third), Some(third)),
        PacketPlan::default()
    );
}
