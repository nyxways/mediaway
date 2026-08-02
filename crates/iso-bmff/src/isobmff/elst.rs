//! Edit list (`elst` / `edts`) — parse + sample expansion (ISOBMFF edit list resolution).

#![forbid(unsafe_code)]

use super::StblSample;
use super::header::parse_header;

/// One `elst` entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditListEntry {
    /// Segment duration in **movie** (`mvhd`) timescale.
    pub segment_duration: u64,
    /// Start media time in **media** (`mdhd`) timescale, or `-1` for an empty edit.
    pub media_time: i64,
    /// Media rate as 16.16 fixed (usually `0x0001_0000`).
    pub media_rate: i32,
}

/// Parse an `elst` box body (version/flags + entries).
///
/// Clamps `entry_count` to the bytes available.
#[must_use]
pub fn parse_elst(body: &[u8]) -> Vec<EditListEntry> {
    if body.len() < 8 {
        return Vec::new();
    }
    let version = body[0];
    let declared = u32::from_be_bytes([body[4], body[5], body[6], body[7]]) as usize;
    let entry_size: usize = if version == 1 { 20 } else { 12 };
    let max_entries = body.len().saturating_sub(8) / entry_size;
    let count = declared.min(max_entries);
    let mut out = Vec::with_capacity(count);
    let mut o = 8;
    for _ in 0..count {
        if o + entry_size > body.len() {
            break;
        }
        let (segment_duration, media_time, media_rate, next) = if version == 1 {
            let dur = u64::from_be_bytes([
                body[o],
                body[o + 1],
                body[o + 2],
                body[o + 3],
                body[o + 4],
                body[o + 5],
                body[o + 6],
                body[o + 7],
            ]);
            let mt = i64::from_be_bytes([
                body[o + 8],
                body[o + 9],
                body[o + 10],
                body[o + 11],
                body[o + 12],
                body[o + 13],
                body[o + 14],
                body[o + 15],
            ]);
            let rate = i32::from_be_bytes([body[o + 16], body[o + 17], body[o + 18], body[o + 19]]);
            (dur, mt, rate, o + 20)
        } else {
            let dur = u64::from(u32::from_be_bytes([
                body[o],
                body[o + 1],
                body[o + 2],
                body[o + 3],
            ]));
            let mt = i64::from(i32::from_be_bytes([
                body[o + 4],
                body[o + 5],
                body[o + 6],
                body[o + 7],
            ]));
            let rate = i32::from_be_bytes([body[o + 8], body[o + 9], body[o + 10], body[o + 11]]);
            (dur, mt, rate, o + 12)
        };
        out.push(EditListEntry {
            segment_duration,
            media_time,
            media_rate,
        });
        o = next;
    }
    out
}

/// Parse `edts` container payload for a nested `elst`.
#[must_use]
pub fn parse_edts(edts_payload: &[u8]) -> Vec<EditListEntry> {
    let mut pos = 0;
    while pos + 8 <= edts_payload.len() {
        let Some(hdr) = parse_header(&edts_payload[pos..]) else {
            break;
        };
        if pos + hdr.size > edts_payload.len() {
            break;
        }
        if &hdr.typ.0 == b"elst" {
            return parse_elst(&edts_payload[pos + hdr.header_len..pos + hdr.size]);
        }
        pos += hdr.size;
    }
    Vec::new()
}

/// Expand raw `stbl` samples according to edit list (presentation order / duplication).
///
/// Per edit, seeks back to a prior keyframe and emits samples through the first keyframe
/// at/after the edit end (includes leading out-of-range decode dependencies).
/// Empty edits (`media_time == -1`) only advance the presentation timeline.
///
/// Timestamps are remapped onto the presentation clock:
/// `dts' = dts - media_time + presentation_base` (may be negative). Samples with
/// composition time outside `[media_time, media_end)` are marked [`StblSample::discard`].
#[must_use]
pub fn expand_samples_by_edit_list(
    raw: &[StblSample],
    edits: &[EditListEntry],
    media_timescale: u32,
    movie_timescale: u32,
) -> Vec<StblSample> {
    if raw.is_empty() || edits.is_empty() || movie_timescale == 0 || media_timescale == 0 {
        return raw.to_vec();
    }

    let mut out = Vec::new();
    let mut presentation_base: i64 = 0;

    for edit in edits {
        let media_dur = rescale_u64(edit.segment_duration, media_timescale, movie_timescale) as i64;
        let edit_base = presentation_base;
        presentation_base = presentation_base.saturating_add(media_dur);

        if edit.media_time < 0 {
            continue;
        }
        let media_time = edit.media_time;
        let media_end = media_time.saturating_add(media_dur);
        let start = find_edit_start_index(raw, media_time);

        for s in raw.iter().skip(start) {
            let cts = sample_cts(s);

            let discard = cts < media_time || cts >= media_end;
            let new_dts = s.dts.saturating_sub(media_time).saturating_add(edit_base);
            out.push(StblSample {
                offset: s.offset,
                size: s.size,
                dts: new_dts,
                duration: s.duration,
                cto: s.cto,
                key: s.key,
                discard,
            });

            // Half-open media window `[media_time, media_end)`. Keep decode-order
            // frames through the keyframe that lands on `media_end`, and stop at the
            // next keyframe with CTS strictly after the window.
            if cts > media_end && s.key {
                break;
            }
        }
    }

    if out.is_empty() { raw.to_vec() } else { out }
}

fn sample_cts(s: &StblSample) -> i64 {
    s.dts.saturating_add(i64::from(s.cto))
}

fn find_edit_start_index(raw: &[StblSample], media_time: i64) -> usize {
    // First sample with CTS >= media_time, then walk back to a keyframe (decode deps).
    let mut idx = raw
        .iter()
        .position(|s| sample_cts(s) >= media_time)
        .unwrap_or(0);
    while idx > 0 && !raw[idx].key {
        idx -= 1;
    }
    idx
}

fn rescale_u64(val: u64, to_scale: u32, from_scale: u32) -> u64 {
    if from_scale == 0 {
        return 0;
    }
    (u128::from(val) * u128::from(to_scale) / u128::from(from_scale)) as u64
}

#[cfg(test)]
#[path = "elst_tests.rs"]
mod tests;
