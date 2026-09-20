//! Replay ring — the last *N* seconds of encoded packets, cut only where a decoder can start.
//!
//! Sans-io bookkeeping over [`Packet`]s: no I/O, no clock. The caller pushes packets as an
//! encoder produces them, and asks for "the last *N* seconds" when someone wants a clip. The
//! answer is a packet sequence a fresh muxer can write as a standalone file.
//!
//! Decisions and their reasons: `crates/mediaway-container/adr/0004-replay-ring.md`. In short:
//!
//! - **One anchor stream** (normally video) decides where a clip may start: only at one of
//!   its keyframes. Everything else (audio) is cut by time to match.
//! - **Decode order, not presentation order.** Eviction and cut selection use `dts`. A clip
//!   starts at keyframe *K*, keeps anchor packets decoded from *K* on, and drops the leading
//!   pictures that present before *K* (their references may be on the far side of the cut).
//! - **Whole GOPs are evicted**, and only once the *next* keyframe is also out of the window,
//!   so a cut point always exists at or before the window's start. A clip can therefore begin
//!   up to one keyframe interval earlier than asked; it never begins later.
//! - **Memory is bounded** by an optional byte ceiling that overrides the window.
//!
//! Payloads are [`Bytes`](mediaway_common::Bytes), so a clip costs a refcount per packet, not
//! a copy of the stream.

use std::collections::VecDeque;
use std::time::Duration;

use mediaway_common::{Packet, Rational};

/// Why the ring refused something.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ReplayError {
    /// A packet for a stream that was never added.
    #[error("stream {stream_id} was not added to the replay ring")]
    UnknownStream {
        /// The packet's stream.
        stream_id: u32,
    },
    /// [`ReplayRing::add_stream`] for a stream that is already there.
    #[error("stream {stream_id} is already in the replay ring")]
    DuplicateStream {
        /// The stream.
        stream_id: u32,
    },
    /// A timebase with a zero numerator or denominator.
    #[error("stream {stream_id} has a degenerate timebase {num}/{den}")]
    BadTimebase {
        /// The stream.
        stream_id: u32,
        /// Numerator.
        num: u64,
        /// Denominator.
        den: u32,
    },
    /// A stream's decode timestamps went backwards. The ring is decode-ordered per stream, and
    /// accepting this would silently break both eviction and cutting.
    #[error("stream {stream_id}: dts {got} after {previous}; packets must arrive in decode order")]
    OutOfOrder {
        /// The stream.
        stream_id: u32,
        /// The last dts accepted on it.
        previous: i64,
        /// The dts that was refused.
        got: i64,
    },
}

/// Nanoseconds on a shared axis, so streams with different timebases can be compared.
type Nanos = i128;

const NANOS_PER_SEC: i128 = 1_000_000_000;

/// `ticks` in `timebase` as nanoseconds, rounded toward negative infinity.
fn to_nanos(ticks: i64, timebase: Rational) -> Nanos {
    (i128::from(ticks) * i128::from(timebase.num) * NANOS_PER_SEC)
        .div_euclid(i128::from(timebase.den))
}

/// `nanos` as ticks in `timebase`, rounded to nearest.
fn to_ticks(nanos: Nanos, timebase: Rational) -> i64 {
    let scale = i128::from(timebase.num) * NANOS_PER_SEC;
    let ticks = (nanos * i128::from(timebase.den) + scale / 2).div_euclid(scale);
    i64::try_from(ticks).unwrap_or(if ticks < 0 { i64::MIN } else { i64::MAX })
}

fn duration_nanos(d: Duration) -> Nanos {
    Nanos::try_from(d.as_nanos()).unwrap_or(Nanos::MAX)
}

/// A decode-ordered queue of one stream's packets.
#[derive(Debug)]
struct Track {
    id: u32,
    timebase: Rational,
    packets: VecDeque<Packet>,
}

impl Track {
    fn last_dts(&self) -> Option<i64> {
        self.packets.back().map(|p| p.dts)
    }
}

/// A keyframe of the anchor stream: somewhere a clip may start.
#[derive(Debug, Clone, Copy)]
struct CutPoint {
    dts: i64,
    pts: i64,
}

/// The last *N* seconds of encoded packets for one or more streams.
///
/// ```
/// use std::time::Duration;
/// use mediaway_common::{Bytes, Packet, Rational};
/// use mediaway_container::replay::ReplayRing;
///
/// let ms = Rational::new(1, 1000);
/// let mut ring = ReplayRing::new(1, ms, Duration::from_secs(10))?;
/// for i in 0..20 {
///     ring.push(Packet {
///         stream_id: 1,
///         pts: i * 1000,
///         dts: i * 1000,
///         duration: 1000,
///         is_keyframe: i % 5 == 0,
///         is_discard: false,
///         payload: Bytes::from_static(b"frame"),
///     })?;
/// }
/// // Asking for 3 s starts at the keyframe before t=16 s: t=15 s, rebased to zero.
/// let clip = ring.clip_last(Duration::from_secs(3)).expect("a keyframe exists");
/// let dts: Vec<i64> = clip.packets().map(|p| p.dts).collect();
/// assert_eq!(dts, [0, 1000, 2000, 3000, 4000]);
/// # Ok::<(), mediaway_container::replay::ReplayError>(())
/// ```
#[derive(Debug)]
pub struct ReplayRing {
    /// `tracks[0]` is the anchor. A `Vec`, not a `SmallVec`: it is allocated once, when the
    /// ring is built, and never on the packet path.
    tracks: Vec<Track>,
    /// The anchor's keyframes still in the ring, oldest first.
    cuts: VecDeque<CutPoint>,
    window: Nanos,
    max_bytes: Option<usize>,
    /// Payload bytes currently held, across every stream.
    bytes: usize,
    /// The newest decode time pushed on any stream.
    newest: Option<Nanos>,
}

impl ReplayRing {
    /// A ring keeping `window` of history, cut at keyframes of stream `anchor`.
    ///
    /// # Errors
    ///
    /// [`ReplayError::BadTimebase`] for a zero numerator or denominator.
    pub fn new(anchor: u32, timebase: Rational, window: Duration) -> Result<Self, ReplayError> {
        let mut ring = Self {
            tracks: Vec::with_capacity(2),
            cuts: VecDeque::new(),
            window: duration_nanos(window),
            max_bytes: None,
            bytes: 0,
            newest: None,
        };
        ring.add_stream(anchor, timebase)?;
        Ok(ring)
    }

    /// Also evict history, oldest GOP first, while more than `max_bytes` of payload is held.
    ///
    /// The newest GOP is always kept, so a single GOP larger than the ceiling can exceed it.
    #[must_use]
    pub const fn with_max_bytes(mut self, max_bytes: usize) -> Self {
        self.max_bytes = Some(max_bytes);
        self
    }

    /// Carry another stream (e.g. audio), cut by time to match the anchor.
    ///
    /// # Errors
    ///
    /// [`ReplayError::DuplicateStream`] or [`ReplayError::BadTimebase`].
    pub fn add_stream(&mut self, stream_id: u32, timebase: Rational) -> Result<(), ReplayError> {
        if timebase.num == 0 || timebase.den == 0 {
            return Err(ReplayError::BadTimebase {
                stream_id,
                num: timebase.num,
                den: timebase.den,
            });
        }
        if self.tracks.iter().any(|t| t.id == stream_id) {
            return Err(ReplayError::DuplicateStream { stream_id });
        }
        self.tracks.push(Track {
            id: stream_id,
            timebase,
            packets: VecDeque::new(),
        });
        Ok(())
    }

    /// Add a packet, then evict whatever has fallen out of the window or over the ceiling.
    ///
    /// Anchor packets that arrive before the anchor's first keyframe are dropped: nothing can
    /// be decoded from them, so no clip could ever use them.
    ///
    /// # Errors
    ///
    /// [`ReplayError::UnknownStream`], or [`ReplayError::OutOfOrder`] if this stream's `dts`
    /// went backwards. The packet is not added in either case.
    pub fn push(&mut self, packet: Packet) -> Result<(), ReplayError> {
        let index = self
            .tracks
            .iter()
            .position(|t| t.id == packet.stream_id)
            .ok_or(ReplayError::UnknownStream {
                stream_id: packet.stream_id,
            })?;
        let track = &mut self.tracks[index];
        if let Some(previous) = track.last_dts()
            && packet.dts < previous
        {
            return Err(ReplayError::OutOfOrder {
                stream_id: packet.stream_id,
                previous,
                got: packet.dts,
            });
        }
        if index == 0 {
            if packet.is_keyframe {
                self.cuts.push_back(CutPoint {
                    dts: packet.dts,
                    pts: packet.pts,
                });
            } else if self.cuts.is_empty() {
                return Ok(());
            }
        }
        let at = to_nanos(packet.dts, track.timebase);
        self.newest = Some(self.newest.map_or(at, |n| n.max(at)));
        self.bytes += packet.payload.len();
        track.packets.push_back(packet);
        self.evict();
        Ok(())
    }

    /// Drop whole GOPs from the front while the second-oldest cut point is itself old enough
    /// to start a full-window clip, or while over the byte ceiling.
    fn evict(&mut self) {
        let Some(newest) = self.newest else { return };
        let anchor = self.tracks[0].timebase;
        while let Some(&next) = self.cuts.get(1) {
            let out_of_window = newest - to_nanos(next.dts, anchor) >= self.window;
            let over_budget = self.max_bytes.is_some_and(|max| self.bytes > max);
            if !(out_of_window || over_budget) {
                break;
            }
            self.cuts.pop_front();
            let (anchor_track, others) = self.tracks.split_at_mut(1);
            let anchor_track = &mut anchor_track[0];
            while anchor_track
                .packets
                .front()
                .is_some_and(|p| p.dts < next.dts)
            {
                if let Some(p) = anchor_track.packets.pop_front() {
                    self.bytes -= p.payload.len();
                }
            }
            let start = to_nanos(next.pts, anchor);
            for track in others {
                let tb = track.timebase;
                while track
                    .packets
                    .front()
                    .is_some_and(|p| to_nanos(p.pts, tb) < start)
                {
                    if let Some(p) = track.packets.pop_front() {
                        self.bytes -= p.payload.len();
                    }
                }
            }
        }
    }

    /// Payload bytes currently held.
    #[must_use]
    pub const fn bytes(&self) -> usize {
        self.bytes
    }

    /// How far back the oldest cut point is from the newest packet, i.e. the longest clip
    /// [`Self::clip_last`] can return right now. Zero before the first keyframe.
    #[must_use]
    pub fn span(&self) -> Duration {
        match (self.cuts.front(), self.newest) {
            (Some(first), Some(newest)) => {
                let nanos = newest - to_nanos(first.dts, self.tracks[0].timebase);
                Duration::from_nanos(u64::try_from(nanos.max(0)).unwrap_or(u64::MAX))
            }
            _ => Duration::ZERO,
        }
    }

    /// The last `span` of every stream, starting at the latest anchor keyframe at or before
    /// `newest - span`. Starts earlier than asked by up to one keyframe interval; starts at
    /// the oldest keyframe held when the ring holds less than `span`.
    ///
    /// `None` until the anchor's first keyframe has been pushed.
    #[must_use]
    pub fn clip_last(&self, span: Duration) -> Option<Clip<'_>> {
        let newest = self.newest?;
        let anchor = self.tracks[0].timebase;
        let target = newest.saturating_sub(duration_nanos(span));
        let later = self
            .cuts
            .partition_point(|c| to_nanos(c.dts, anchor) <= target);
        let cut = *self.cuts.get(later.saturating_sub(1))?;
        Some(Clip {
            ring: self,
            cut,
            origin: to_nanos(cut.dts, anchor),
        })
    }
}

/// A cut of a [`ReplayRing`]: which packets, and their rebased timestamps.
///
/// Borrows the ring, so it has to be written out before the next [`ReplayRing::push`].
#[derive(Debug, Clone, Copy)]
pub struct Clip<'a> {
    ring: &'a ReplayRing,
    cut: CutPoint,
    /// The cut keyframe's decode time. Becomes zero in the clip.
    origin: Nanos,
}

impl<'a> Clip<'a> {
    /// The cut keyframe's `dts`, in the anchor's own timebase and on the timeline the packets
    /// were pushed on. Subtracting it maps a time on that timeline into the clip.
    #[must_use]
    pub const fn anchor_origin(&self) -> i64 {
        self.cut.dts
    }

    /// From the cut to the newest packet pushed.
    #[must_use]
    pub fn duration(&self) -> Duration {
        let nanos = self.ring.newest.unwrap_or(self.origin) - self.origin;
        Duration::from_nanos(u64::try_from(nanos.max(0)).unwrap_or(u64::MAX))
    }

    /// The clip's packets, interleaved across streams in decode-time order, with `pts` and
    /// `dts` rebased so the cut keyframe decodes at zero.
    ///
    /// Each item clones a [`Packet`], whose payload is a refcounted `Bytes`: no payload copy.
    #[must_use]
    pub fn packets(&self) -> ClipPackets<'a> {
        let ring = self.ring;
        let cut_pts = to_nanos(self.cut.pts, ring.tracks[0].timebase);
        let cursors = ring
            .tracks
            .iter()
            .enumerate()
            .map(|(i, t)| {
                if i == 0 {
                    t.packets.partition_point(|p| p.dts < self.cut.dts)
                } else {
                    t.packets
                        .partition_point(|p| to_nanos(p.dts, t.timebase) < cut_pts)
                }
            })
            .collect();
        ClipPackets {
            ring,
            cut: self.cut,
            origin: self.origin,
            cursors,
        }
    }
}

/// Iterator returned by [`Clip::packets`].
#[derive(Debug)]
pub struct ClipPackets<'a> {
    ring: &'a ReplayRing,
    cut: CutPoint,
    origin: Nanos,
    /// Next index per track. Allocated once per clip, not per packet.
    cursors: Vec<usize>,
}

impl Iterator for ClipPackets<'_> {
    type Item = Packet;

    fn next(&mut self) -> Option<Packet> {
        loop {
            // The track whose next packet decodes first. Ties go to the lower index, so the
            // anchor comes first.
            let (track, packet) = self
                .ring
                .tracks
                .iter()
                .enumerate()
                .filter_map(|(i, t)| t.packets.get(self.cursors[i]).map(|p| (i, p)))
                .min_by_key(|&(i, p)| (to_nanos(p.dts, self.ring.tracks[i].timebase), i))?;
            self.cursors[track] += 1;
            // A leading picture of the cut: decoded after it, presented before it, and free to
            // reference the GOP that was cut away. Other streams need no such check: their
            // cursors start at the first packet decoded at or after the cut's presentation
            // time, and a packet never presents before it decodes.
            if track == 0 && packet.pts < self.cut.pts {
                continue;
            }
            let timebase = self.ring.tracks[track].timebase;
            let offset = if track == 0 {
                self.cut.dts
            } else {
                to_ticks(self.origin, timebase)
            };
            // clone: the ring keeps its copy for later clips; the payload is a refcounted
            // `Bytes`, so this is a refcount bump, not a copy of the frame.
            let mut out = packet.clone();
            out.pts -= offset;
            out.dts -= offset;
            return Some(out);
        }
    }
}

#[cfg(test)]
#[path = "replay_tests.rs"]
mod tests;
