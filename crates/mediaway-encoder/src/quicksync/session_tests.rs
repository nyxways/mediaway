//! Tests for [`super::QuickSyncSession`] — see `docs/conventions/testing.md` Tier 1.
//!
//! Hardware-verified 2026-07-29 against this workspace's reference Windows
//! box's real Intel UHD 770 (`libmfxhw64.dll`) — see this crate's
//! `adr/0001-onevpl-quicksync-encode-surface.md` 2026-07-29 addendum for the
//! full `--nocapture` output. Written to skip honestly (never panic) on a
//! host with no oneVPL runtime, since this crate must also stay usable there.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "test modules may unwrap / print"
)]

use super::*;
use mediaway_common::{CodecKind, PixelFormat, Rational, VideoFrameStorage};
use vpl_sys::VplError;

#[test]
fn align16_rounds_up_to_next_multiple_of_16() {
    assert_eq!(align16(0), 0);
    assert_eq!(align16(1), 16);
    assert_eq!(align16(16), 16);
    assert_eq!(align16(17), 32);
    assert_eq!(align16(1080), 1088);
    assert_eq!(align16(1920), 1920);
}

#[test]
fn ts_90k_roundtrip_is_stable_at_30fps() {
    let time_base = Rational::new(1, 30);
    for units in [0i64, 1, 2, 30, 300, 12_345] {
        let ticks = ts_to_90k(time_base, units);
        let back = ts_from_90k(time_base, ticks);
        assert_eq!(back, units, "roundtrip mismatch for units={units}");
    }
}

/// Synthetic NV12 test frame: a Y-plane gradient (so real encode has actual
/// signal to compress, not a flat/degenerate image) plus neutral (128) UV.
fn synthetic_nv12_frame(width: u32, height: u32, frame_index: u8) -> Bytes {
    let w = width as usize;
    let h = height as usize;
    let mut buf = vec![0u8; w * h + w * h / 2];
    for y in 0..h {
        for x in 0..w {
            #[allow(
                clippy::cast_possible_truncation,
                reason = "modulo 256 by construction fits u8"
            )]
            let value = ((x + y + usize::from(frame_index)) % 256) as u8;
            buf[y * w + x] = value;
        }
    }
    for b in &mut buf[w * h..] {
        *b = 128;
    }
    Bytes::from(buf)
}

/// Scan Annex-B `payload` for NAL start codes (`00 00 01` / `00 00 00 01`)
/// and return the set of `nal_unit_type` values found (low 5 bits of the
/// byte immediately after each start code, per H.264 Annex B / ITU-T H.264
/// §7.3.1).
fn annex_b_nal_types(payload: &[u8]) -> Vec<u8> {
    let mut types = Vec::new();
    let mut i = 0usize;
    while i + 3 < payload.len() {
        let is_start3 = payload[i] == 0 && payload[i + 1] == 0 && payload[i + 2] == 1;
        let is_start4 = i + 4 < payload.len()
            && payload[i] == 0
            && payload[i + 1] == 0
            && payload[i + 2] == 0
            && payload[i + 3] == 1;
        if is_start4 {
            types.push(payload[i + 4] & 0x1f);
            i += 5;
        } else if is_start3 {
            types.push(payload[i + 3] & 0x1f);
            i += 4;
        } else {
            i += 1;
        }
    }
    types
}

/// Number of synthetic frames pushed by [`real_encode_produces_annex_b_sps_and_idr_or_skips`].
const FRAME_COUNT: u8 = 15;

/// Opens a real oneVPL H.264 CPU-upload session, encodes a handful of
/// synthetic NV12 frames, flushes, and verifies the collected Annex-B
/// bitstream actually contains an SPS (`nal_unit_type == 7`) and an IDR
/// slice (`nal_unit_type == 5`) — i.e. a real, decodable-shaped H.264 stream,
/// not just "some bytes came out". Skips (does not fail) when no oneVPL
/// implementation is available on this host.
#[test]
fn real_encode_produces_annex_b_sps_and_idr_or_skips() {
    let width = 176u32;
    let height = 144u32;
    let config = VideoEncoderConfig {
        codec: CodecKind::H264,
        width,
        height,
        time_base: Rational::new(1, 30),
        bitrate_bps: 500_000,
        pixel_format: PixelFormat::Nv12,
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: None,
        gop_size: 1,
        rate_control: None,
        intra_refresh_period: None,
    };

    let mut session = match QuickSyncSession::open(&config) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "skip: QuickSyncSession::open failed ({e:?}) — no oneVPL runtime on this host?"
            );
            return;
        }
    };

    for i in 0..FRAME_COUNT {
        let frame = VideoFrame {
            pts: i64::from(i),
            duration: 1,
            width,
            height,
            format: PixelFormat::Nv12,
            storage: VideoFrameStorage::Cpu {
                data: synthetic_nv12_frame(width, height, i),
            },
        };
        session
            .push_frame(&frame)
            .expect("push_frame should succeed against real hardware");
    }
    session.flush().expect("flush should succeed");

    let mut packets = Vec::new();
    while let Some(p) = session.poll_packet().expect("poll_packet should not error") {
        packets.push(p);
    }

    assert!(
        !packets.is_empty(),
        "real oneVPL encode should have produced at least one packet for {FRAME_COUNT} frames"
    );
    assert!(
        packets[0].is_keyframe,
        "the first emitted packet should be a keyframe (IDR)"
    );

    let mut all_nal_types = Vec::new();
    for p in &packets {
        all_nal_types.extend(annex_b_nal_types(&p.payload));
    }

    eprintln!(
        "vpl-sys/mediaway-encoder-quicksync: real encode produced {} packet(s), NAL types seen: {all_nal_types:?}",
        packets.len()
    );

    assert!(
        all_nal_types.contains(&7),
        "expected an SPS NAL (type 7) somewhere in the real encoded output; saw {all_nal_types:?}"
    );
    assert!(
        all_nal_types.contains(&5),
        "expected an IDR slice NAL (type 5) somewhere in the real encoded output; saw {all_nal_types:?}"
    );
}

/// Scan Annex-B `payload` for NAL start codes and return the set of HEVC
/// `nal_unit_type` values found (bits 1-6 of the first header byte — HEVC
/// uses a 2-byte NAL header, ITU-T H.265 §7.3.1.2 — unlike H.264's 1-byte,
/// low-5-bits header [`annex_b_nal_types`] reads).
fn annex_b_nal_types_hevc(payload: &[u8]) -> Vec<u8> {
    let mut types = Vec::new();
    let mut i = 0usize;
    while i + 3 < payload.len() {
        let is_start3 = payload[i] == 0 && payload[i + 1] == 0 && payload[i + 2] == 1;
        let is_start4 = i + 4 < payload.len()
            && payload[i] == 0
            && payload[i + 1] == 0
            && payload[i + 2] == 0
            && payload[i + 3] == 1;
        if is_start4 {
            types.push((payload[i + 4] >> 1) & 0x3f);
            i += 5;
        } else if is_start3 {
            types.push((payload[i + 3] >> 1) & 0x3f);
            i += 4;
        } else {
            i += 1;
        }
    }
    types
}

/// Opens a real oneVPL HEVC CPU-upload session (mirrors
/// [`real_encode_produces_annex_b_sps_and_idr_or_skips`] above, HEVC instead
/// of H.264), encodes synthetic NV12 frames, flushes, and verifies the
/// collected Annex-B bitstream actually contains an SPS (`nal_unit_type ==
/// 33`) and an IDR slice (`nal_unit_type == 19` `IDR_W_RADL` or `20`
/// `IDR_N_LP`) — i.e. a real, decodable-shaped HEVC stream. Skips (does not
/// fail) when no oneVPL implementation is available, or when this hardware's
/// driver genuinely does not support HEVC encode (unexpected on Xe/UHD-
/// generation Intel iGPUs, but this test does not assume that).
#[test]
fn real_hevc_encode_produces_vps_sps_pps_idr_or_skips() {
    let width = 176u32;
    let height = 144u32;
    let config = VideoEncoderConfig {
        codec: CodecKind::Hevc,
        width,
        height,
        time_base: Rational::new(1, 30),
        bitrate_bps: 500_000,
        pixel_format: PixelFormat::Nv12,
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: None,
        gop_size: 1,
        rate_control: None,
        intra_refresh_period: None,
    };

    let mut session = match QuickSyncSession::open(&config) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "skip: QuickSyncSession::open (HEVC) failed ({e:?}) — no oneVPL runtime, or this \
                 hardware/driver does not support HEVC encode?"
            );
            return;
        }
    };

    for i in 0..FRAME_COUNT {
        let frame = VideoFrame {
            pts: i64::from(i),
            duration: 1,
            width,
            height,
            format: PixelFormat::Nv12,
            storage: VideoFrameStorage::Cpu {
                data: synthetic_nv12_frame(width, height, i),
            },
        };
        session
            .push_frame(&frame)
            .expect("push_frame should succeed against real hardware");
    }
    session.flush().expect("flush should succeed");

    let mut packets = Vec::new();
    while let Some(p) = session.poll_packet().expect("poll_packet should not error") {
        packets.push(p);
    }

    assert!(
        !packets.is_empty(),
        "real oneVPL HEVC encode should have produced at least one packet for {FRAME_COUNT} frames"
    );
    assert!(
        packets[0].is_keyframe,
        "the first emitted HEVC packet should be a keyframe (IDR)"
    );

    let mut all_nal_types = Vec::new();
    for p in &packets {
        all_nal_types.extend(annex_b_nal_types_hevc(&p.payload));
    }

    eprintln!(
        "vpl-sys/mediaway-encoder-quicksync: real HEVC encode produced {} packet(s), NAL types seen: {all_nal_types:?}",
        packets.len()
    );

    assert!(
        all_nal_types.contains(&33),
        "expected an SPS NAL (type 33) somewhere in the real encoded HEVC output; saw {all_nal_types:?}"
    );
    assert!(
        all_nal_types.contains(&19) || all_nal_types.contains(&20),
        "expected an IDR slice NAL (type 19 or 20) somewhere in the real encoded HEVC output; saw {all_nal_types:?}"
    );
}

/// Attempts real AV1 encode (`MFXVideoENCODE_Query` then, if that succeeds,
/// `MFXVideoENCODE_Init`) against this workspace's real Intel UHD 770 (Alder
/// Lake, Xe-LP) — a generation not documented as supporting AV1 hardware
/// *encode* (AV1 *decode* is a separate, already-supported capability).
///
/// Talks to `vpl-sys` directly (not through [`QuickSyncSession::open`], whose
/// [`EncodeError`] intentionally does not carry the underlying `mfxStatus` —
/// "details in logs when available" per that type's docs) so a genuine
/// failure's exact status code is captured and printed here, in the logs.
/// This is the expected, valuable, honest result this crate's `adr/0001`
/// 2026-07-29 HEVC/AV1 addendum documents — not forced or faked either way:
/// if this hardware/driver combination unexpectedly supports AV1 encode,
/// that success (and `MFXVideoENCODE_Init` result) is reported too. Skips
/// (does not fail) only when no oneVPL runtime is present on this host at
/// all — every other outcome (`Query`/`Init` success or a real `mfxStatus`
/// error) is treated as a valid, asserted-on result, not a skip.
#[test]
fn av1_encode_query_reports_real_hardware_result_or_skips() {
    let width = 176u32;
    let height = 144u32;
    let config = VideoEncoderConfig {
        codec: CodecKind::Av1,
        width,
        height,
        time_base: Rational::new(1, 30),
        bitrate_bps: 500_000,
        pixel_format: PixelFormat::Nv12,
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: None,
        gop_size: 1,
        rate_control: None,
        intra_refresh_period: None,
    };
    validate(&config).expect("AV1 config should pass this crate's generic input validation");

    let loader = match Loader::open() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("skip: Loader::open failed ({e}) — no oneVPL runtime on this host?");
            return;
        }
    };
    let mut session = match loader.create_session(MFX_IMPL_HARDWARE_ANY) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "skip: create_session(MFX_IMPL_HARDWARE_ANY) failed ({e}) — no oneVPL runtime on \
                 this host?"
            );
            return;
        }
    };

    let aligned_width = align16(width);
    let aligned_height = align16(height);
    let frame_info = build_frame_info(&config, aligned_width, aligned_height);
    let (codec_id, codec_profile, codec_level) =
        codec_params(CodecKind::Av1).expect("AV1 has codec params in this crate's scope");
    let mfx = build_mfx_info(
        frame_info,
        config.bitrate_bps,
        codec_id,
        codec_profile,
        codec_level,
    );
    let mut params = mfxVideoParam {
        AsyncDepth: 1,
        __bindgen_anon_1: mfxVideoParam__bindgen_ty_1 { mfx },
        IOPattern: MFX_IOPATTERN_IN_SYSTEM_MEMORY,
        ..Default::default()
    };

    // `Session::encode_query` only ever returns `Ok` or `VplError::Status` (see
    // `vpl-sys::dispatcher`'s implementation) — no separate dispatcher-level
    // failure arm is needed here (those were already handled above, at
    // `Loader::open`/`create_session`).
    let query_status = match session.encode_query(&mut params) {
        Ok(status) => status,
        Err(e) => {
            eprintln!(
                "vpl-sys/mediaway-encoder-quicksync: real MFXVideoENCODE_Query(AV1) failed on \
                 this Alder Lake / Xe-LP hardware/driver — {e}"
            );
            return;
        }
    };

    eprintln!(
        "vpl-sys/mediaway-encoder-quicksync: real MFXVideoENCODE_Query(AV1) SUCCEEDED \
         (mfxStatus={query_status}) on this Intel UHD 770 (Alder Lake / Xe-LP) — unexpected per \
         this generation's documented capabilities; attempting MFXVideoENCODE_Init next"
    );

    let init_result = session.encode_init(&mut params);
    assert!(
        matches!(init_result, Ok(()) | Err(VplError::Status { .. })),
        "expected either a real success or a real Status error from MFXVideoENCODE_Init(AV1), \
         got {init_result:?}"
    );
    match init_result {
        Ok(()) => {
            eprintln!(
                "vpl-sys/mediaway-encoder-quicksync: real MFXVideoENCODE_Init(AV1) UNEXPECTEDLY \
                 SUCCEEDED on this hardware/driver — genuine AV1 hardware encode capability found; \
                 a full push_frame/GOP path is not built by this diagnostic test"
            );
            let _ = session.encode_close();
        }
        Err(e) => {
            eprintln!(
                "vpl-sys/mediaway-encoder-quicksync: real MFXVideoENCODE_Init(AV1) failed on this \
                 Alder Lake / Xe-LP hardware/driver — {e}"
            );
        }
    }
}
