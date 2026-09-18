//! Tier 7 — the end-to-end oracle this crate did not have for HEVC.
//!
//! # Why a separate file, and why it uses ffmpeg as an input source
//!
//! Every HEVC test in this crate before 2026-09-18 fed the muxer hand-written bytes — the
//! longest was six of them — and all of them passed while a mediaway-muxed HEVC MP4 decoded
//! **zero** frames in any player. Synthetic payloads cannot catch that class of bug, because
//! the bug lives in the relationship between three things a fixture does not have: a real
//! SPS with emulation-prevention escapes in its `profile_tier_level()`, real NAL framing, and
//! a decoder willing to complain.
//!
//! So this test takes real HEVC from ffmpeg, strips it back to Annex-B, pushes it through
//! *this* crate's muxer with **no** out-of-band configuration record, and asks ffprobe to
//! decode the result. That exercises all three defects fixed in `adr/0006-hevc-in-mp4.md` at
//! once:
//!
//! 1. `build_hvcc` must unescape the SPS, or `general_level_idc` ships as `0`.
//! 2. `Muxer::push_packet` must length-prefix HEVC samples, or `mdat` and `hvcC` disagree.
//! 3. The record must be backfilled from the first packet, since nothing supplies one here.
//!
//! It skips — loudly, never silently — when ffmpeg, ffprobe, or an HEVC encoder is missing.
//! A machine without them is not a machine this test has an opinion about.

#![forbid(unsafe_code)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr,
    reason = "oracle tests may unwrap / panic / skip-log"
)]

use std::path::{Path, PathBuf};
use std::process::Command;

use bytes::Bytes;
use iso_bmff::bitstream::hevc::{
    annex_b_sequence_header, hvcc_payload_to_annex_b, parse_hevc_decoder_config,
};
use iso_bmff::{Codec, Demuxer, Muxer, Rational, Sample, Track};

/// One access unit: whether it is a random-access point, and its Annex-B bytes.
type AnnexBUnit = (bool, Bytes);

/// A decoded fixture: pixel dimensions plus its access units, in decode order.
type Fixture = (u32, u32, Vec<AnnexBUnit>);

/// Frames the fixture clip contains.
const FIXTURE_FRAMES: usize = 30;

/// Whether a tool answers `-version` successfully.
fn tool_available(tool: &str) -> bool {
    Command::new(tool)
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// A temp file that removes itself, so a failing test leaves no clips behind.
struct TempFile(PathBuf);

impl TempFile {
    fn new(name: &str) -> Self {
        Self(std::env::temp_dir().join(name))
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn str(&self) -> &str {
        self.0.to_str().expect("utf8 temp path")
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Encode a one-second HEVC clip with ffmpeg, or `None` if no HEVC encoder is available.
fn encode_fixture(out: &TempFile) -> Option<()> {
    // `-g 15` so the clip has more than one GOP: a stream that is all-IDR would not exercise
    // the reference handling a real recording depends on.
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x240:rate=30:duration=1",
            "-c:v",
            "libx265",
            "-preset",
            "ultrafast",
            "-g",
            "15",
            "-pix_fmt",
            "yuv420p",
            "-x265-params",
            "log-level=none",
            out.str(),
        ])
        .output()
        .ok()?;
    if !status.status.success() {
        eprintln!(
            "skip hevc oracle: ffmpeg could not encode HEVC ({})",
            String::from_utf8_lossy(&status.stderr).trim()
        );
        return None;
    }
    Some(())
}

/// Demux ffmpeg's MP4 back into Annex-B access units plus the track's real dimensions.
///
/// Returns Annex-B rather than the length-prefixed samples the demuxer yields, because
/// Annex-B is what an encoder hands a muxer in practice — it is the input shape the bug lived
/// in. The parameter sets go in front of the first access unit, which is exactly what a WMF or
/// NVENC MFT emits.
fn to_annex_b_units(mp4: &[u8]) -> Option<Fixture> {
    let mut demux = Demuxer::new();
    demux.push_bytes(mp4);
    let stream = demux.streams().first()?.clone();
    if stream.codec != Codec::Hevc {
        eprintln!("skip hevc oracle: ffmpeg produced {:?}", stream.codec);
        return None;
    }
    let config = parse_hevc_decoder_config(&stream.extra_data)?;
    let header = annex_b_sequence_header(&config);

    let mut units = Vec::new();
    while let Some(packet) = demux.poll_packet() {
        let annex = hvcc_payload_to_annex_b(&packet.payload, config.nal_length_size);
        let payload = if units.is_empty() {
            let mut first = Vec::with_capacity(header.len() + annex.len());
            first.extend_from_slice(&header);
            first.extend_from_slice(&annex);
            Bytes::from(first)
        } else {
            annex
        };
        units.push((packet.is_keyframe, payload));
    }
    (!units.is_empty()).then_some((stream.width, stream.height, units))
}

/// Mux Annex-B access units with **no** out-of-band configuration record.
fn mux_without_a_config_record(width: u32, height: u32, units: &[AnnexBUnit]) -> Vec<u8> {
    let mut open = Muxer::with_fragment_batch(8);
    open.add_track(Track {
        id: 0,
        codec: Codec::Hevc,
        time_base: Rational::new(1, 30),
        width,
        height,
        // Empty on purpose: this is the case where the encoder backend published nothing, and
        // the muxer has to recover the record from the first packet's parameter sets.
        extra_data: Bytes::new(),
    })
    .expect("track");
    let mut mux = open.begin();

    for (index, (is_keyframe, payload)) in units.iter().enumerate() {
        // One tick per frame. `try_from` cannot fail for a one-second clip, and saturating is
        // a better answer than a panic in a test whose subject is elsewhere.
        let tick = i64::try_from(index).unwrap_or(i64::MAX);
        mux.push_packet(&Sample {
            stream_id: 0,
            pts: tick,
            dts: tick,
            duration: 1,
            is_keyframe: *is_keyframe,
            is_discard: false,
            // clone: refcounted payload, not a bitstream copy
            payload: payload.clone(),
        })
        .expect("push");
    }
    mux.flush();

    let mut bytes = Vec::new();
    mux.poll_bytes(&mut bytes);
    bytes
}

/// Frames ffprobe can actually decode out of a file.
fn decodable_frames(path: &Path) -> Result<usize, String> {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-count_frames",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=nb_read_frames",
            "-of",
            "default=nw=1:nk=1",
            path.to_str().expect("utf8 path"),
        ])
        .output()
        .map_err(|e| e.to_string())?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !stderr.trim().is_empty() {
        return Err(stderr.trim().to_owned());
    }
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<usize>()
        .map_err(|e| format!("unparseable frame count: {e}"))
}

#[test]
fn ffprobe_decodes_every_frame_of_a_mediaway_muxed_hevc_file() {
    if !tool_available("ffmpeg") || !tool_available("ffprobe") {
        eprintln!("skip hevc oracle: ffmpeg/ffprobe not on PATH");
        return;
    }
    let source = TempFile::new("mediaway_hevc_oracle_source.mp4");
    if encode_fixture(&source).is_none() {
        return;
    }
    let source_bytes = std::fs::read(source.path()).expect("read fixture");
    let Some((width, height, units)) = to_annex_b_units(&source_bytes) else {
        eprintln!("skip hevc oracle: could not recover Annex-B units from the fixture");
        return;
    };
    assert_eq!(
        units.len(),
        FIXTURE_FRAMES,
        "the fixture should be one second at 30 fps"
    );

    let muxed = mux_without_a_config_record(width, height, &units);

    // Assert the record before asking ffprobe, so a failure says *which* defect came back
    // rather than just "the file does not play".
    let config = extract_hvcc(&muxed).expect("an hvcC box in the written file");
    let parsed = parse_hevc_decoder_config(&config).expect("hvcC must parse");
    assert!(
        !parsed.vps.is_empty() && !parsed.sps.is_empty() && !parsed.pps.is_empty(),
        "hvcC carries no parameter sets — the placeholder was written instead of a real record"
    );
    assert_ne!(
        config[12], 0,
        "general_level_idc is 0: the SPS was read without unescaping it"
    );

    let out = TempFile::new("mediaway_hevc_oracle_muxed.mp4");
    std::fs::write(out.path(), &muxed).expect("write muxed");
    match decodable_frames(out.path()) {
        Ok(frames) => assert_eq!(
            frames, FIXTURE_FRAMES,
            "ffprobe decoded {frames} of {FIXTURE_FRAMES} frames"
        ),
        Err(e) => panic!("ffprobe could not read the muxed file: {e}"),
    }
}

/// The `hvcC` box payload out of a muxed file.
///
/// A flat scan rather than a box walk: `hvcC` is nested four levels down
/// (`moov/trak/mdia/minf/stbl/stsd/hvc1/hvcC`) and this test only needs the payload, not the
/// path to it.
fn extract_hvcc(buf: &[u8]) -> Option<Vec<u8>> {
    let at = buf.windows(4).position(|w| w == b"hvcC")?;
    // The four bytes before the type are the box size, including the 8-byte header.
    let size_at = at.checked_sub(4)?;
    let size = usize::try_from(u32::from_be_bytes(buf[size_at..at].try_into().ok()?)).ok()?;
    let payload_start = at + 4;
    let payload_end = size_at.checked_add(size)?;
    (payload_end <= buf.len() && payload_end > payload_start)
        .then(|| buf[payload_start..payload_end].to_vec())
}
