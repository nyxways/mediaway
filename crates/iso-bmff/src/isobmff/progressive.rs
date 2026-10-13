//! A non-fragmented MP4 (`ftyp`, `moov` with real sample tables, `mdat`) for samples that are all
//! known before the file is written — the writing counterpart of [`super::stbl::parse_stbl_samples`].
//!
//! A fragmented file ([`super::moov::write_moov`]) is what a muxer writes while samples keep
//! coming. A clip cut from packets already in hand is better off progressive for one reason: an
//! edit list. `FFmpeg`'s demuxer applies an `elst` to the sample tables of a non-fragmented file
//! (it trims the samples before `media_time` and starts the presentation there) and says
//! "`advanced_editlist` does not work with fragmented MP4. disabling." for a fragmented one, where it
//! only shifts the timestamps and shows everything. So a stream copy that starts at the keyframe
//! before the moment it should show from — and hides the packets in between with an edit list —
//! is written here.
//!
//! The header comes first (`moov` before `mdat`, so a player can start from the first bytes), its
//! size known before a single payload byte is written because everything it holds (sizes, times,
//! flags) is: [`write_progressive`] writes `ftyp`, `moov` and the `mdat` box header; the caller then
//! writes the `payload_len` payload bytes, samples in the order of their `offset`s.

#![forbid(unsafe_code)]

use super::elst::EditListEntry;
use super::moov::{MOVIE_TIMESCALE, write_mvhd, write_trak};
use super::{FourCc, StblSample, tag, write_box, write_ftyp};
use crate::types::Track;

/// One track of a non-fragmented file.
#[derive(Debug, Clone, Copy)]
pub struct ProgressiveTrack<'a> {
    /// Codec, timescale (`time_base.den`), size and configuration record.
    pub track: &'a Track,
    /// The track's samples in decode order, each one its own chunk. `offset` is the sample's
    /// place from the start of the `mdat` payload; `dts`, `duration` and `cto` are media ticks
    /// (the first `dts` is normally zero: the `stts` carries durations only).
    pub samples: &'a [StblSample],
    /// The track's edit list (none for no `edts`), see [`EditListEntry`]: `segment_duration` in
    /// [`MOVIE_TIMESCALE`] ticks, `media_time` in the track's ticks, `-1` for an empty edit (a
    /// track that starts after the movie does: its first entry is empty, as long as the wait).
    pub edits: &'a [EditListEntry],
}

/// Write the head of a non-fragmented MP4: `ftyp`, `moov` and the header of the `mdat` box.
///
/// The `mdat` payload is `payload_len` bytes, which the caller writes next. The `moov`'s chunk offsets are
/// absolute (the sample's `offset` plus the head's length), as 64-bit `co64` when the file is
/// over 4 GiB and 32-bit `stco` otherwise.
///
/// Sample durations come from `StblSample::duration`, and the track's and the movie's lengths
/// from the samples (or the edit list when there is one). A track with no samples is written
/// with empty tables.
pub fn write_progressive(buf: &mut Vec<u8>, tracks: &[ProgressiveTrack<'_>], payload_len: u64) {
    let owned: Vec<Track> = tracks.iter().map(|t| t.track.clone()).collect();
    let mut ftyp = Vec::new();
    write_ftyp(&mut ftyp, &owned);
    // A payload past 4 GiB (less the header) needs the 64-bit box size.
    let mdat_header: u64 = if payload_len.saturating_add(8) > u64::from(u32::MAX) {
        16
    } else {
        8
    };
    // The size of the `moov` depends on the width of the offsets in it, and those on its size:
    // measure with 32-bit offsets and widen them if the file turns out to need it. Sizes do not
    // depend on the offsets' values, so the last pass is as long as the one measured.
    let mut wide = false;
    let (moov, _) = loop {
        let mut measured = Vec::new();
        write_moov(&mut measured, tracks, 0, wide);
        let base = (ftyp.len() + measured.len()) as u64 + mdat_header;
        if !wide && base.saturating_add(payload_len) > u64::from(u32::MAX) {
            wide = true;
            continue;
        }
        let mut moov = Vec::with_capacity(measured.len());
        write_moov(&mut moov, tracks, base, wide);
        break (moov, base);
    };
    buf.extend_from_slice(&ftyp);
    buf.extend_from_slice(&moov);
    if mdat_header == 16 {
        buf.extend_from_slice(&1u32.to_be_bytes());
        buf.extend_from_slice(&tag::MDAT.0);
        buf.extend_from_slice(&payload_len.saturating_add(16).to_be_bytes());
    } else {
        buf.extend_from_slice(
            &u32::try_from(payload_len + 8)
                .unwrap_or(u32::MAX)
                .to_be_bytes(),
        );
        buf.extend_from_slice(&tag::MDAT.0);
    }
}

/// `moov` with the sample tables: chunk offsets are `base` plus each sample's `offset`, `wide`
/// for `co64`.
fn write_moov(buf: &mut Vec<u8>, tracks: &[ProgressiveTrack<'_>], base: u64, wide: bool) {
    let movie = tracks
        .iter()
        .map(|t| {
            let tables = Tables::new(t.samples, 0, false);
            if t.edits.is_empty() {
                tables.movie_duration(t.track.time_base.den)
            } else {
                t.edits.iter().map(|e| e.segment_duration).sum()
            }
        })
        .max()
        .unwrap_or(0);
    write_box(buf, tag::MOOV, |b| {
        write_mvhd(b, tracks.len(), u32::try_from(movie).unwrap_or(u32::MAX));
        for t in tracks {
            let tables = Tables::new(t.samples, base, wide);
            write_trak(b, t.track, t.edits, Some(&tables));
        }
    });
}

/// The sample tables of one track (`stts`, `ctts`, `stss`, `stsc`, `stsz`, `stco`/`co64`).
#[derive(Debug, Clone, Copy)]
pub(super) struct Tables<'a> {
    samples: &'a [StblSample],
    base: u64,
    wide: bool,
}

impl<'a> Tables<'a> {
    const fn new(samples: &'a [StblSample], base: u64, wide: bool) -> Self {
        Self {
            samples,
            base,
            wide,
        }
    }

    /// The sum of the samples' durations, media ticks.
    pub(super) fn media_duration(&self) -> u64 {
        self.samples.iter().map(|s| u64::from(s.duration)).sum()
    }

    /// The same in [`MOVIE_TIMESCALE`] ticks, for a track of `timescale` ticks a second, rounded up.
    pub(super) fn movie_duration(&self, timescale: u32) -> u64 {
        let ticks = u128::from(self.media_duration()) * u128::from(MOVIE_TIMESCALE);
        let scale = u128::from(timescale.max(1));
        u64::try_from(ticks.div_ceil(scale)).unwrap_or(u64::MAX)
    }

    /// The boxes, after the `stsd`.
    pub(super) fn write(&self, st: &mut Vec<u8>) {
        let count = u32::try_from(self.samples.len()).unwrap_or(u32::MAX);
        write_box(st, FourCc(*b"stts"), |b| {
            let runs = runs(self.samples.iter().map(|s| i64::from(s.duration)));
            full_box_header(b, 0, runs.len());
            for (n, duration) in runs {
                b.extend_from_slice(&n.to_be_bytes());
                b.extend_from_slice(&u32::try_from(duration).unwrap_or(0).to_be_bytes());
            }
        });
        if self.samples.iter().any(|s| s.cto != 0) {
            write_box(st, FourCc(*b"ctts"), |b| {
                // Version 1 has signed offsets; 0 would read a negative one as huge.
                let signed = self.samples.iter().any(|s| s.cto < 0);
                let runs = runs(self.samples.iter().map(|s| i64::from(s.cto)));
                full_box_header(b, u8::from(signed), runs.len());
                for (n, offset) in runs {
                    b.extend_from_slice(&n.to_be_bytes());
                    b.extend_from_slice(&i32::try_from(offset).unwrap_or(0).to_be_bytes());
                }
            });
        }
        if self.samples.iter().any(|s| !s.key) {
            write_box(st, FourCc(*b"stss"), |b| {
                let keys: Vec<u32> = (1..=count)
                    .zip(self.samples)
                    .filter(|(_, s)| s.key)
                    .map(|(n, _)| n)
                    .collect();
                full_box_header(b, 0, keys.len());
                for n in keys {
                    b.extend_from_slice(&n.to_be_bytes());
                }
            });
        }
        // One sample to a chunk: the chunk offsets are the samples' own.
        write_box(st, FourCc(*b"stsc"), |b| {
            full_box_header(b, 0, usize::from(count > 0));
            if count > 0 {
                for field in [1u32, 1, 1] {
                    b.extend_from_slice(&field.to_be_bytes());
                }
            }
        });
        write_box(st, FourCc(*b"stsz"), |b| {
            b.extend_from_slice(&0u32.to_be_bytes()); // version/flags
            b.extend_from_slice(&0u32.to_be_bytes()); // sample_size = 0: a size per sample
            b.extend_from_slice(&count.to_be_bytes());
            for s in self.samples {
                b.extend_from_slice(&s.size.to_be_bytes());
            }
        });
        let tag = if self.wide { *b"co64" } else { *b"stco" };
        write_box(st, FourCc(tag), |b| {
            full_box_header(b, 0, self.samples.len());
            for s in self.samples {
                let at = self.base.saturating_add(s.offset);
                if self.wide {
                    b.extend_from_slice(&at.to_be_bytes());
                } else {
                    b.extend_from_slice(&u32::try_from(at).unwrap_or(u32::MAX).to_be_bytes());
                }
            }
        });
    }
}

/// Version and flags, then the entry count.
fn full_box_header(b: &mut Vec<u8>, version: u8, entries: usize) {
    b.extend_from_slice(&(u32::from(version) << 24).to_be_bytes());
    b.extend_from_slice(&u32::try_from(entries).unwrap_or(u32::MAX).to_be_bytes());
}

/// `values` as `(how many in a row, value)`.
fn runs(values: impl Iterator<Item = i64>) -> Vec<(u32, i64)> {
    let mut out: Vec<(u32, i64)> = Vec::new();
    for v in values {
        match out.last_mut() {
            Some((n, last)) if *last == v => *n += 1,
            _ => out.push((1, v)),
        }
    }
    out
}

#[cfg(test)]
#[path = "progressive_tests.rs"]
mod tests;
