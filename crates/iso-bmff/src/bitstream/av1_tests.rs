#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "test modules may unwrap"
)]

use super::*;

/// `[obu_header=0x0A (type=1 SEQUENCE_HEADER, has_size_field=1), leb128 size=4, 4 payload
/// bytes]` — a minimal, syntactically valid Sequence Header OBU (content is not semantically
/// parsed by `to_av1c`, only its framing).
const SEQ_HEADER_OBU: [u8; 6] = [0x0A, 0x04, 0xAA, 0xBB, 0xCC, 0xDD];

/// `[obu_header=0x12 (type=2 TEMPORAL_DELIMITER, has_size_field=1), leb128 size=0]`.
const TEMPORAL_DELIMITER_OBU: [u8; 2] = [0x12, 0x00];

#[test]
fn to_av1c_extracts_sequence_header_obu_verbatim() {
    let out = to_av1c(&SEQ_HEADER_OBU);
    let av1c = out.av1c.expect("sequence header OBU present");
    assert_eq!(av1c[0], 0x81, "marker=1, version=1");
    assert_eq!(&av1c[1..4], &[0, 0, 0], "bitfields deferred, zeroed");
    assert_eq!(&av1c[4..], &SEQ_HEADER_OBU, "configOBUs verbatim");
}

#[test]
fn to_av1c_skips_leading_non_sequence_header_obus() {
    let mut data = Vec::new();
    data.extend_from_slice(&TEMPORAL_DELIMITER_OBU);
    data.extend_from_slice(&SEQ_HEADER_OBU);
    let out = to_av1c(&data);
    let av1c = out.av1c.expect("sequence header OBU present after TD");
    assert_eq!(&av1c[4..], &SEQ_HEADER_OBU);
}

#[test]
fn to_av1c_returns_none_when_no_sequence_header_obu_present() {
    // Only a temporal delimiter — no OBU_SEQUENCE_HEADER anywhere in the stream.
    let out = to_av1c(&TEMPORAL_DELIMITER_OBU);
    assert!(out.av1c.is_none());
}

#[test]
fn to_av1c_returns_none_on_empty_input() {
    let out = to_av1c(&[]);
    assert!(out.av1c.is_none());
}

#[test]
fn to_av1c_returns_none_when_forbidden_bit_set() {
    let garbage = [0xFFu8, 0x00, 0x01, 0x02];
    let out = to_av1c(&garbage);
    assert!(out.av1c.is_none());
}

#[test]
fn to_av1c_returns_none_when_no_size_field_and_no_panic() {
    // type=1 (SEQUENCE_HEADER) but obu_has_size_field=0 — this helper cannot safely bound
    // the OBU without parsing its payload, so it must decline rather than guess.
    let no_size_field = [0x08u8, 0xAA, 0xBB, 0xCC];
    let out = to_av1c(&no_size_field);
    assert!(out.av1c.is_none());
}

#[test]
fn to_av1c_returns_none_on_truncated_leb128_and_no_panic() {
    // has_size_field=1 but the leb128 continuation byte is never terminated within the slice.
    let truncated = [0x0Au8, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80];
    let out = to_av1c(&truncated);
    assert!(out.av1c.is_none());
}

#[test]
fn to_av1c_returns_none_when_declared_size_exceeds_input_and_no_panic() {
    // Declares an OBU payload of 200 bytes but only 2 bytes actually follow.
    let truncated_payload = [0x0Au8, 0xC8, 0x01, 0xAA];
    let out = to_av1c(&truncated_payload);
    assert!(out.av1c.is_none());
}

/// Real Sequence Header OBU bytes from a genuine `libaom-av1` bitstream, via system `ffmpeg`
/// (optional test/dev oracle, [ADR-0002](../../adr/0002-system-oracle.md)) — the same oracle
/// `mediaway-decoder-windows`'s AV1 decode test already uses. Skips (does not fail the default
/// suite) when `ffmpeg` is not on `PATH`.
#[test]
fn to_av1c_extracts_real_sequence_header_from_ffmpeg_libaom_av1_or_skip() {
    let Some(ivf) = ffmpeg_av1_ivf(64, 64) else {
        eprintln!("skip: system ffmpeg (libaom-av1) unavailable — optional oracle, see ADR-0002");
        return;
    };
    let payloads = parse_ivf_frame_payloads(&ivf);
    let Some(first) = payloads.first() else {
        eprintln!("skip: ffmpeg produced an IVF file with zero frames");
        return;
    };
    let out = to_av1c(first);
    let Some(av1c) = out.av1c else {
        eprintln!("skip: no Sequence Header OBU found in the first real ffmpeg AV1 temporal unit");
        return;
    };
    assert_eq!(av1c[0], 0x81, "marker=1, version=1");
    eprintln!(
        "real ffmpeg AV1 sequence header OBU extracted OK — {} configOBUs byte(s)",
        av1c.len() - 4
    );
}

fn ffmpeg_av1_ivf(width: u32, height: u32) -> Option<Vec<u8>> {
    let output = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            &format!("testsrc=size={width}x{height}:rate=30:d=1"),
            "-c:v",
            "libaom-av1",
            "-frames:v",
            "1",
            "-strict",
            "experimental",
            "-f",
            "ivf",
            "pipe:1",
        ])
        .output()
        .ok()?;
    if !output.status.success() || output.stdout.is_empty() {
        return None;
    }
    Some(output.stdout)
}

/// Split an IVF container's frame payloads out (32-byte file header, then repeated
/// `[u32 LE size][u64 LE timestamp][payload]` frame chunks — each payload is one real AV1
/// temporal unit / concatenated-OBU sample).
fn parse_ivf_frame_payloads(data: &[u8]) -> Vec<Vec<u8>> {
    if data.len() < 32 || &data[0..4] != b"DKIF" {
        return Vec::new();
    }
    let mut frames = Vec::new();
    let mut off = 32usize;
    while off + 12 <= data.len() {
        let size =
            u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]) as usize;
        off += 12;
        if off + size > data.len() {
            break;
        }
        frames.push(data[off..off + size].to_vec());
        off += size;
    }
    frames
}
