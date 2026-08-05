//! Tests for [`super::NvencSession`] — see `docs/conventions/testing.md` Tier 1.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "test modules may unwrap / print"
)]
#![allow(
    clippy::similar_names,
    reason = "first_packet_has_vps/sps/sequence_header read clearly side by side in these tests"
)]

use super::*;

/// NVENC's D3D11 device-type session rejects very small requests (empirically confirmed:
/// 64x64 fails `init_encoder` with `NVencError::InvalidParam` on this machine's RTX 4090;
/// 640x480 succeeds) — use a comfortably safe size for the hardware-gated test below rather
/// than guessing an exact driver-defined minimum.
fn h264_cfg(width: u32, height: u32) -> VideoEncoderConfig {
    VideoEncoderConfig {
        codec: CodecKind::H264,
        width,
        height,
        time_base: Rational::new(1, 30),
        bitrate_bps: 2_000_000,
        pixel_format: PixelFormat::Nv12,
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: None,
        gop_size: 1,
        rate_control: None,
    }
}

fn hevc_cfg(width: u32, height: u32) -> VideoEncoderConfig {
    VideoEncoderConfig {
        codec: CodecKind::Hevc,
        ..h264_cfg(width, height)
    }
}

fn av1_cfg(width: u32, height: u32) -> VideoEncoderConfig {
    VideoEncoderConfig {
        codec: CodecKind::Av1,
        ..h264_cfg(width, height)
    }
}

/// 5 synthetic NV12 frames (slightly different luma per frame, neutral mid-gray chroma) —
/// shared shape for every hardware-gated codec test below.
fn synthetic_nv12_frames(width: u32, height: u32) -> Vec<VideoFrame> {
    let nv12_len = (width * height) as usize + (width * height) as usize / 2;
    (0..5u8)
        .map(|i| {
            let mut data = vec![64u8 + i * 20; nv12_len];
            for b in &mut data[(width * height) as usize..] {
                *b = 128;
            }
            VideoFrame {
                pts: i64::from(i),
                duration: 1,
                width,
                height,
                format: PixelFormat::Nv12,
                storage: VideoFrameStorage::Cpu {
                    data: Bytes::from(data),
                },
            }
        })
        .collect()
}

// --- Pure logic: no D3D11/NVENC session needed --------------------------------------------

#[test]
fn validate_accepts_even_h264_cpu_upload_config() {
    assert!(validate(&h264_cfg(640, 480)).is_ok());
}

#[test]
fn validate_accepts_hevc_cpu_upload_config() {
    let mut cfg = h264_cfg(640, 480);
    cfg.codec = CodecKind::Hevc;
    assert!(validate(&cfg).is_ok());
}

#[test]
fn validate_accepts_av1_cpu_upload_config() {
    let mut cfg = h264_cfg(640, 480);
    cfg.codec = CodecKind::Av1;
    assert!(validate(&cfg).is_ok());
}

/// NVENC has no VP9 **encoder** at all (VP9 is decode-only on this silicon, see ADR-0001) —
/// the one `CodecKind` this backend can never support regardless of GPU/driver.
#[test]
fn validate_rejects_vp9_codec() {
    let mut cfg = h264_cfg(640, 480);
    cfg.codec = CodecKind::Vp9;
    assert_eq!(validate(&cfg), Err(EncodeError::Unsupported));
}

#[test]
fn validate_rejects_zero_copy_gpu_input() {
    let mut cfg = h264_cfg(640, 480);
    cfg.input = VideoInputPreference::ZeroCopyGpu;
    assert_eq!(validate(&cfg), Err(EncodeError::Unsupported));
}

#[test]
fn validate_rejects_zero_dimensions() {
    let cfg = h264_cfg(0, 480);
    assert_eq!(validate(&cfg), Err(EncodeError::InvalidInput));
}

#[test]
fn validate_rejects_odd_dimensions() {
    let cfg = h264_cfg(641, 480);
    assert_eq!(validate(&cfg), Err(EncodeError::InvalidInput));
}

#[test]
fn validate_rejects_non_nv12_pixel_format() {
    let mut cfg = h264_cfg(640, 480);
    cfg.pixel_format = PixelFormat::Bgra8;
    assert_eq!(validate(&cfg), Err(EncodeError::Unsupported));
}

#[test]
fn validate_rejects_zero_timebase_denominator() {
    let mut cfg = h264_cfg(640, 480);
    cfg.time_base = Rational::new(1, 0);
    assert_eq!(validate(&cfg), Err(EncodeError::InvalidInput));
}

#[test]
fn frame_rate_divides_den_by_num() {
    assert_eq!(frame_rate(Rational::new(1, 30)), [30, 1]);
    assert_eq!(frame_rate(Rational::new(1001, 30_000)), [30_000, 1001]);
}

#[test]
fn frame_rate_clamps_zero_numerator_to_one() {
    assert_eq!(frame_rate(Rational::new(0, 60)), [60, 1]);
}

#[test]
fn contains_h264_idr_nal_finds_type_5_after_start_code() {
    // 00 00 00 01 65 == start code + NAL header with nal_unit_type = 5 (IDR slice).
    let data = [0x00, 0x00, 0x00, 0x01, 0x65, 0xAA, 0xBB];
    assert!(contains_h264_idr_nal(&data));
}

#[test]
fn contains_h264_idr_nal_false_for_non_idr_slice() {
    // NAL type 1 (non-IDR slice / P-frame).
    let data = [0x00, 0x00, 0x00, 0x01, 0x41, 0xAA, 0xBB];
    assert!(!contains_h264_idr_nal(&data));
}

#[test]
fn contains_hevc_idr_nal_finds_idr_w_radl_after_start_code() {
    // 00 00 00 01 <hdr0> <hdr1> — HEVC NAL header: forbidden(1)=0, nal_unit_type(6)=19
    // (IDR_W_RADL), layer_id/tid bits arbitrary (2-byte header, only the first byte's top
    // 6 bits (after the forbidden bit) matter for this scan). 19 << 1 == 0x26.
    let data = [0x00, 0x00, 0x00, 0x01, 0x26, 0x01, 0xAA, 0xBB];
    assert!(contains_hevc_idr_nal(&data));
}

#[test]
fn contains_hevc_idr_nal_finds_idr_n_lp_after_start_code() {
    // nal_unit_type = 20 (IDR_N_LP): 20 << 1 == 0x28.
    let data = [0x00, 0x00, 0x00, 0x01, 0x28, 0x01, 0xAA, 0xBB];
    assert!(contains_hevc_idr_nal(&data));
}

#[test]
fn contains_hevc_idr_nal_false_for_trail_r_slice() {
    // nal_unit_type = 1 (TRAIL_R, a non-IDR trailing picture): 1 << 1 == 0x02.
    let data = [0x00, 0x00, 0x00, 0x01, 0x02, 0x01, 0xAA, 0xBB];
    assert!(!contains_hevc_idr_nal(&data));
}

#[test]
fn read_leb128_decodes_single_byte_value() {
    // 0x05 has the continuation bit (0x80) unset — a single-byte leb128 encoding of 5.
    assert_eq!(read_leb128(&[0x05, 0xFF]), Some((5, 1)));
}

#[test]
fn read_leb128_decodes_multi_byte_value() {
    // 0xE5 0x8E 0x26 == 624_485 (the canonical leb128 example from the WASM/AV1 spec text).
    assert_eq!(read_leb128(&[0xE5, 0x8E, 0x26]), Some((624_485, 3)));
}

#[test]
fn read_leb128_none_when_truncated() {
    // Continuation bit set on every byte, buffer ends without a terminating byte.
    assert_eq!(read_leb128(&[0x80, 0x80, 0x80]), None);
}

#[test]
fn contains_av1_sequence_header_obu_finds_type_1() {
    // OBU header byte: forbidden(1)=0, obu_type(4)=1 (OBU_SEQUENCE_HEADER), extension(1)=0,
    // has_size_field(1)=1, reserved(1)=0 => 0b0_0001_0_1_0 == 0x0A. Size field: leb128 `0x02`
    // (2 payload bytes) then 2 arbitrary payload bytes.
    let data = [0x0A, 0x02, 0xAA, 0xBB];
    assert!(contains_av1_sequence_header_obu(&data));
}

#[test]
fn contains_av1_sequence_header_obu_skips_temporal_delimiter_to_find_sequence_header() {
    // OBU_TEMPORAL_DELIMITER (type 2): 0b0_0010_0_1_0 == 0x12, size 0 (empty leb128 `0x00`).
    // Then OBU_SEQUENCE_HEADER (type 1) as above.
    let data = [0x12, 0x00, 0x0A, 0x02, 0xAA, 0xBB];
    assert!(contains_av1_sequence_header_obu(&data));
}

#[test]
fn contains_av1_sequence_header_obu_false_without_sequence_header() {
    // OBU_TEMPORAL_DELIMITER (type 2) followed by OBU_FRAME (type 6): 0b0_0110_0_1_0 == 0x32.
    let data = [0x12, 0x00, 0x32, 0x02, 0xAA, 0xBB];
    assert!(!contains_av1_sequence_header_obu(&data));
}

#[test]
fn stream_info_from_config_carries_geometry_and_timebase() {
    let cfg = h264_cfg(640, 480);
    let info = stream_info_from(&cfg);
    assert!(matches!(
        info,
        StreamInfo::Video {
            codec: CodecKind::H264,
            ..
        }
    ));
    if let StreamInfo::Video {
        time_base,
        geometry,
        ..
    } = info
    {
        assert_eq!(time_base, cfg.time_base);
        assert_eq!(geometry.width, 640);
        assert_eq!(geometry.height, 480);
    }
}

// --- Hardware-gated: real NVENC session -----------------------------------------------------

/// Opens a real NVENC H.264 CPU-upload session and encodes several synthetic NV12 frames
/// end to end (private D3D11 device → staging-texture upload → `CopyResource` →
/// `register_resource_dx11` → `encode_picture` → bitstream lock), verifying real Annex-B
/// H.264 output with SPS/PPS + an IDR slice on the first frame.
///
/// **Hardware-verified 2026-07-29 on a real NVIDIA `GeForce` RTX 4090** (driver 32.0.15.9579,
/// `nvEncodeAPI64.dll` present) — see [ADR-0001](../../adr/0001-nvenc-vendor-backend.md)
/// 2026-07-29 addendum. Skips gracefully (does not fail the suite) when no NVENC-capable
/// GPU/driver is available, so `cargo test --workspace` stays green on other machines.
#[test]
fn nvenc_open_and_encode_or_skip_without_hw() {
    const WIDTH: u32 = 640;
    const HEIGHT: u32 = 480;
    let cfg = h264_cfg(WIDTH, HEIGHT);

    let mut enc = match NvencSession::open(&cfg) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("skip: NvencSession::open failed ({e:?}) — no NVENC-capable GPU/driver?");
            return;
        }
    };

    let nv12_len = (WIDTH * HEIGHT) as usize + (WIDTH * HEIGHT) as usize / 2;
    let mut packets_emitted = 0usize;
    let mut first_packet_keyframe = false;
    let mut first_packet_has_sps = false;

    for i in 0..5u8 {
        // Slightly different luma value per frame so this is not a degenerate all-zero input.
        let mut data = vec![64u8 + i * 20; nv12_len];
        // Neutral chroma (mid-gray, no color cast).
        for b in &mut data[(WIDTH * HEIGHT) as usize..] {
            *b = 128;
        }
        let frame = VideoFrame {
            pts: i64::from(i),
            duration: 1,
            width: WIDTH,
            height: HEIGHT,
            format: PixelFormat::Nv12,
            storage: VideoFrameStorage::Cpu {
                data: Bytes::from(data),
            },
        };

        if let Err(e) = enc.push_frame(&frame) {
            eprintln!("skip: push_frame failed ({e:?}) — no usable NVENC session?");
            return;
        }

        while let Ok(Some(p)) = enc.poll_packet() {
            assert!(!p.payload.is_empty(), "packet {packets_emitted} is empty");
            let has_start_code = p.payload.windows(4).any(|w| w == [0, 0, 0, 1]);
            assert!(
                has_start_code,
                "packet {packets_emitted} has no Annex-B start code"
            );
            if packets_emitted == 0 {
                first_packet_keyframe = p.is_keyframe;
                first_packet_has_sps = p
                    .payload
                    .windows(5)
                    .any(|w| w[..4] == [0, 0, 0, 1] && (w[4] & 0x1F) == 7);
            }
            packets_emitted += 1;
        }
    }

    let _ = enc.flush();
    while let Ok(Some(p)) = enc.poll_packet() {
        assert!(!p.payload.is_empty());
        packets_emitted += 1;
    }

    assert!(packets_emitted >= 1, "expected at least one encoded packet");
    assert!(
        first_packet_keyframe,
        "first packet should be an IDR keyframe"
    );
    assert!(
        first_packet_has_sps,
        "first packet should carry an inline SPS NAL"
    );
    eprintln!("nvenc h264 cpu-upload packets={packets_emitted}");
}

/// Opens a real NVENC HEVC CPU-upload session and encodes several synthetic NV12 frames end
/// to end (same D3D11 staging-texture upload path as the H.264 test above), verifying real
/// Annex-B HEVC output with VPS/SPS/PPS + an IDR slice on the first frame.
///
/// **Hardware-verified 2026-07-29 on a real NVIDIA `GeForce` RTX 4090** (driver 32.0.15.9579)
/// — see [ADR-0001](../../adr/0001-nvenc-vendor-backend.md) 2026-07-29 (HEVC/AV1) addendum.
/// Skips gracefully (does not fail the suite) when no NVENC-capable GPU/driver is available.
#[test]
fn nvenc_open_and_encode_hevc_or_skip_without_hw() {
    const WIDTH: u32 = 640;
    const HEIGHT: u32 = 480;
    let cfg = hevc_cfg(WIDTH, HEIGHT);

    let mut enc = match NvencSession::open(&cfg) {
        Ok(e) => e,
        Err(e) => {
            eprintln!(
                "skip: NvencSession::open (HEVC) failed ({e:?}) — no HEVC-capable NVENC GPU/driver?"
            );
            return;
        }
    };

    let mut packets_emitted = 0usize;
    let mut first_packet_keyframe = false;
    let mut first_packet_has_vps = false;
    let mut first_packet_has_sps = false;
    let mut first_packet_dump = String::new();

    for frame in synthetic_nv12_frames(WIDTH, HEIGHT) {
        if let Err(e) = enc.push_frame(&frame) {
            eprintln!("skip: push_frame (HEVC) failed ({e:?}) — no usable NVENC HEVC session?");
            return;
        }

        while let Ok(Some(p)) = enc.poll_packet() {
            assert!(!p.payload.is_empty(), "packet {packets_emitted} is empty");
            let has_start_code = p.payload.windows(4).any(|w| w == [0, 0, 0, 1]);
            assert!(
                has_start_code,
                "packet {packets_emitted} has no Annex-B start code"
            );
            if packets_emitted == 0 {
                first_packet_keyframe = p.is_keyframe;
                first_packet_has_vps = p
                    .payload
                    .windows(5)
                    .any(|w| w[..4] == [0, 0, 0, 1] && (w[4] >> 1) & 0x3F == 32);
                first_packet_has_sps = p
                    .payload
                    .windows(5)
                    .any(|w| w[..4] == [0, 0, 0, 1] && (w[4] >> 1) & 0x3F == 33);
                first_packet_dump = hex_dump(&p.payload[..p.payload.len().min(32)]);
            }
            packets_emitted += 1;
        }
    }

    let _ = enc.flush();
    while let Ok(Some(p)) = enc.poll_packet() {
        assert!(!p.payload.is_empty());
        packets_emitted += 1;
    }

    assert!(packets_emitted >= 1, "expected at least one encoded packet");
    assert!(
        first_packet_keyframe,
        "first packet should be an IDR keyframe"
    );
    assert!(
        first_packet_has_vps,
        "first packet should carry an inline VPS NAL (type 32)"
    );
    assert!(
        first_packet_has_sps,
        "first packet should carry an inline SPS NAL (type 33)"
    );
    eprintln!(
        "nvenc hevc cpu-upload packets={packets_emitted} first_packet_prefix={first_packet_dump}"
    );
}

/// `bytes` as a space-separated lowercase hex string — used only to print real encoded output
/// as evidence in hardware-gated test logs (`--nocapture`), not for parsing.
fn hex_dump(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Opens a real NVENC AV1 CPU-upload session and attempts to encode several synthetic NV12
/// frames end to end. Ada Lovelace (this crate's reference RTX 4090) is NVIDIA-documented as
/// having 8th-gen NVENC with AV1 hardware encode, and the `nvenc` crate's `sys::guids`
/// exposes `NV_ENC_CODEC_AV1_GUID` — this test exists to verify for real whether the crate's
/// generic `Session`/`Encoder` safe API (already codec-agnostic: GUID selection is the only
/// codec-specific input `open()` makes) actually drives AV1 end to end on this hardware, not
/// just whether the GUID constant compiles.
///
/// **Hardware-verified 2026-07-29 on a real NVIDIA `GeForce` RTX 4090** — see
/// [ADR-0001](../../adr/0001-nvenc-vendor-backend.md) 2026-07-29 (HEVC/AV1) addendum for the
/// exact outcome and which layer (bindings vs. hardware/driver) it points to. Skips
/// gracefully (does not fail the suite) when AV1 is unavailable at any layer, printing which
/// call failed and how — this is the honest signal this test is designed to capture.
#[test]
fn nvenc_open_and_encode_av1_or_skip_without_hw() {
    const WIDTH: u32 = 640;
    const HEIGHT: u32 = 480;
    let cfg = av1_cfg(WIDTH, HEIGHT);

    let mut enc = match NvencSession::open(&cfg) {
        Ok(e) => e,
        Err(e) => {
            eprintln!(
                "skip: NvencSession::open (AV1) failed ({e:?}) — no AV1-capable NVENC GPU/driver, \
                 or the `nvenc` crate's generic session/encoder path does not actually drive AV1 \
                 end to end on this hardware/driver"
            );
            return;
        }
    };

    let mut packets_emitted = 0usize;
    let mut first_packet_keyframe = false;
    let mut first_packet_has_sequence_header = false;
    let mut first_packet_dump = String::new();

    for frame in synthetic_nv12_frames(WIDTH, HEIGHT) {
        if let Err(e) = enc.push_frame(&frame) {
            eprintln!(
                "skip: push_frame (AV1) failed ({e:?}) — session opened but AV1 encode_picture did not"
            );
            return;
        }

        while let Ok(Some(p)) = enc.poll_packet() {
            assert!(!p.payload.is_empty(), "packet {packets_emitted} is empty");
            if packets_emitted == 0 {
                first_packet_keyframe = p.is_keyframe;
                first_packet_has_sequence_header = contains_av1_sequence_header_obu(&p.payload);
                first_packet_dump = hex_dump(&p.payload[..p.payload.len().min(32)]);
            }
            packets_emitted += 1;
        }
    }

    let _ = enc.flush();
    while let Ok(Some(p)) = enc.poll_packet() {
        assert!(!p.payload.is_empty());
        packets_emitted += 1;
    }

    assert!(packets_emitted >= 1, "expected at least one encoded packet");
    eprintln!(
        "nvenc av1 cpu-upload packets={packets_emitted} first_packet_keyframe={first_packet_keyframe} \
         first_packet_has_sequence_header_obu={first_packet_has_sequence_header} \
         first_packet_prefix={first_packet_dump}"
    );
    assert!(
        first_packet_has_sequence_header,
        "first packet should carry an OBU_SEQUENCE_HEADER (AV1's keyframe signal)"
    );
}
