//! Probe result shape plus text (`-of default`) and JSON (`-of json`) rendering.
//!
//! JSON is hand-rolled: the flag/field surface here is small and `serde` is
//! not yet a workspace dependency (see `docs/conventions/deps-policy.md`).

use mediaway_common::{CodecKind, Rational};
use std::fmt::Write as _;

/// Per-stream summary derived from Mediaway demux metadata.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StreamSummary {
    pub(crate) index: u32,
    pub(crate) codec: CodecKind,
    pub(crate) width: Option<u32>,
    pub(crate) height: Option<u32>,
    pub(crate) time_base: Rational,
    pub(crate) packet_count: u64,
    /// Derived from `max(pts + duration) - min(pts)` across demuxed packets,
    /// converted via `time_base`. `None` when the stream has no packets.
    pub(crate) duration_seconds: Option<f64>,
}

/// Container-level summary.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FormatSummary {
    pub(crate) format_name: &'static str,
    /// `ftyp` major brand (e.g. `isom`, `mp42`), when the box is present.
    pub(crate) major_brand: Option<String>,
    /// Longest per-stream duration, if any stream yielded one.
    pub(crate) duration_seconds: Option<f64>,
    pub(crate) stream_count: usize,
}

/// Full probe result: format summary plus per-stream summaries.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ProbeReport {
    pub(crate) format: FormatSummary,
    pub(crate) streams: Vec<StreamSummary>,
}

const fn codec_name(codec: CodecKind) -> &'static str {
    match codec {
        CodecKind::H264 => "h264",
        CodecKind::Hevc => "hevc",
        CodecKind::Av1 => "av1",
        CodecKind::Vp9 => "vp9",
        CodecKind::Vp8 => "vp8",
        CodecKind::Aac => "aac",
        CodecKind::Opus => "opus",
        CodecKind::WebVtt => "webvtt",
        CodecKind::Tx3g => "tx3g",
        CodecKind::RawVideo => "rawvideo",
        CodecKind::RawAudio => "rawaudio",
        CodecKind::Mp3 => "mp3",
        CodecKind::Vorbis => "vorbis",
        CodecKind::ProRes422Proxy => "prores422proxy",
        CodecKind::ProRes422Lt => "prores422lt",
        CodecKind::ProRes422 => "prores422",
        CodecKind::ProRes422Hq => "prores422hq",
        CodecKind::ProRes4444 => "prores4444",
        CodecKind::ProRes4444Xq => "prores4444xq",
    }
}

fn opt_u32(v: Option<u32>) -> String {
    v.map_or_else(|| "N/A".to_owned(), |x| x.to_string())
}

fn opt_duration(v: Option<f64>) -> String {
    v.map_or_else(|| "N/A".to_owned(), |x| format!("{x:.3}"))
}

/// Render as ffprobe `-of default`-style `key=value` sections.
pub(crate) fn render_text(report: &ProbeReport, show_format: bool, show_streams: bool) -> String {
    let mut out = String::new();
    if show_streams {
        for s in &report.streams {
            let _ = writeln!(out, "[STREAM]");
            let _ = writeln!(out, "index={}", s.index);
            let _ = writeln!(out, "codec_name={}", codec_name(s.codec));
            let _ = writeln!(out, "width={}", opt_u32(s.width));
            let _ = writeln!(out, "height={}", opt_u32(s.height));
            let _ = writeln!(out, "time_base={}/{}", s.time_base.num, s.time_base.den);
            let _ = writeln!(out, "nb_packets={}", s.packet_count);
            let _ = writeln!(out, "duration={}", opt_duration(s.duration_seconds));
            let _ = writeln!(out, "[/STREAM]");
        }
    }
    if show_format {
        let _ = writeln!(out, "[FORMAT]");
        let _ = writeln!(out, "format_name={}", report.format.format_name);
        let _ = writeln!(
            out,
            "major_brand={}",
            report.format.major_brand.as_deref().unwrap_or("N/A")
        );
        let _ = writeln!(out, "nb_streams={}", report.format.stream_count);
        let _ = writeln!(
            out,
            "duration={}",
            opt_duration(report.format.duration_seconds)
        );
        let _ = writeln!(out, "[/FORMAT]");
    }
    out
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

fn json_opt_u32(v: Option<u32>) -> String {
    v.map_or_else(|| "null".to_owned(), |x| x.to_string())
}

fn json_opt_duration(v: Option<f64>) -> String {
    v.map_or_else(|| "null".to_owned(), |x| format!("{x:.3}"))
}

fn json_opt_string(v: Option<&str>) -> String {
    v.map_or_else(|| "null".to_owned(), |s| format!("\"{}\"", json_escape(s)))
}

fn stream_json(s: &StreamSummary) -> String {
    format!(
        "{{\"index\": {}, \"codec_name\": \"{}\", \"width\": {}, \"height\": {}, \
         \"time_base\": \"{}/{}\", \"nb_packets\": {}, \"duration\": {}}}",
        s.index,
        codec_name(s.codec),
        json_opt_u32(s.width),
        json_opt_u32(s.height),
        s.time_base.num,
        s.time_base.den,
        s.packet_count,
        json_opt_duration(s.duration_seconds),
    )
}

fn format_json(f: &FormatSummary) -> String {
    format!(
        "{{\"format_name\": \"{}\", \"major_brand\": {}, \"nb_streams\": {}, \"duration\": {}}}",
        json_escape(f.format_name),
        json_opt_string(f.major_brand.as_deref()),
        f.stream_count,
        json_opt_duration(f.duration_seconds),
    )
}

/// Render as a JSON object (`-of json`), ffprobe-shaped (`streams`/`format` keys).
pub(crate) fn render_json(report: &ProbeReport, show_format: bool, show_streams: bool) -> String {
    let mut parts = Vec::new();
    if show_streams {
        let streams_json: Vec<String> = report.streams.iter().map(stream_json).collect();
        parts.push(format!("\"streams\": [{}]", streams_json.join(", ")));
    }
    if show_format {
        parts.push(format!("\"format\": {}", format_json(&report.format)));
    }
    format!("{{\n  {}\n}}\n", parts.join(",\n  "))
}

#[cfg(test)]
#[path = "report_tests.rs"]
mod tests;
