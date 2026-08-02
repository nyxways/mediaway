//! Build a [`ProbeReport`] from container bytes via the Mediaway MP4 demux path.
//!
//! Only what `mediaway-container` already exposes is surfaced here: per-stream
//! codec/geometry/timebase from [`mediaway_common::StreamInfo`], plus a
//! best-effort duration derived from demuxed packet timestamps (there is no
//! movie/track-duration box getter in the public API yet).

use crate::error::ProbeError;
use crate::report::{FormatSummary, ProbeReport, StreamSummary};
use mediaway_common::Rational;
use mediaway_container::mp4::{Demuxer, mp4_parser};
use std::path::Path;

/// Only container format currently supported end-to-end.
const FORMAT_NAME: &str = "mp4";

/// Demux `bytes` (read from `path`, kept only for error messages) into a
/// [`ProbeReport`].
///
/// # Errors
///
/// [`ProbeError::Unsupported`] when no streams are discovered (not a
/// recognizable/parseable MP4).
pub(crate) fn build_report(path: &Path, bytes: &[u8]) -> Result<ProbeReport, ProbeError> {
    let major_brand = major_brand(bytes);

    let mut demuxer = Demuxer::new();
    demuxer.push_bytes(bytes);

    let streams = demuxer.streams().to_vec();
    if streams.is_empty() {
        return Err(ProbeError::Unsupported {
            path: path.display().to_string(),
        });
    }

    // (min_pts, max_pts_end, packet_count) per stream, in declaration order.
    let mut extents = vec![(i64::MAX, i64::MIN, 0_u64); streams.len()];
    while let Some(packet) = demuxer.poll_packet() {
        let Some(idx) = streams.iter().position(|s| s.id() == packet.stream_id) else {
            continue;
        };
        let (min_pts, max_end, count) = &mut extents[idx];
        *min_pts = (*min_pts).min(packet.pts);
        let end = packet
            .pts
            .saturating_add(i64::try_from(packet.duration).unwrap_or(i64::MAX));
        *max_end = (*max_end).max(end);
        *count += 1;
    }

    let stream_summaries: Vec<StreamSummary> = streams
        .iter()
        .zip(extents.iter())
        .map(|(s, &(min_pts, max_end, count))| {
            let has_duration = count > 0 && max_end > min_pts;
            let duration_seconds =
                has_duration.then_some(seconds(s.time_base(), max_end - min_pts));
            let geometry = s.geometry();
            StreamSummary {
                index: s.id(),
                codec: s.codec(),
                width: geometry.map(|g| g.width),
                height: geometry.map(|g| g.height),
                time_base: s.time_base(),
                packet_count: count,
                duration_seconds,
            }
        })
        .collect();

    let mut duration_seconds: Option<f64> = None;
    for s in &stream_summaries {
        if let Some(d) = s.duration_seconds {
            duration_seconds = Some(duration_seconds.map_or(d, |acc: f64| acc.max(d)));
        }
    }

    Ok(ProbeReport {
        format: FormatSummary {
            format_name: FORMAT_NAME,
            major_brand,
            duration_seconds,
            stream_count: stream_summaries.len(),
        },
        streams: stream_summaries,
    })
}

// Precision loss for very large tick/num counts is acceptable for a probe summary.
#[allow(clippy::cast_precision_loss)]
fn seconds(time_base: Rational, ticks: i64) -> f64 {
    if time_base.den == 0 || ticks <= 0 {
        return 0.0;
    }
    (ticks as f64) * (time_base.num as f64) / f64::from(time_base.den)
}

fn major_brand(bytes: &[u8]) -> Option<String> {
    let tree = mp4_parser::parse_box_tree(bytes, 0);
    let ftyp = tree.iter().find(|n| &n.header.tag == b"ftyp")?;
    let start = ftyp.payload_offset;
    let brand = bytes.get(start..start + 4)?;
    Some(String::from_utf8_lossy(brand).into_owned())
}
