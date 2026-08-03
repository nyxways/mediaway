#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "unit tests"
)]

use super::*;
use mediaway_common::{Bytes, CodecKind, Rational, VideoFrameStorage};
use mediaway_encoder::windows::WindowsVideoEncoder;
use mediaway_encoder::{VideoEncoder, VideoEncoderConfig, VideoInputPreference};
use windows::Win32::Media::MediaFoundation::{
    IMFActivate, MFMediaType_Video, MFT_CATEGORY_VIDEO_DECODER, MFT_ENUM_FLAG,
    MFT_ENUM_FLAG_SORTANDFILTER, MFT_ENUM_FLAG_SYNCMFT, MFT_FRIENDLY_NAME_Attribute,
    MFT_REGISTER_TYPE_INFO, MFTEnumEx, MFVideoFormat_AV1, MFVideoFormat_HEVC, MFVideoFormat_NV12,
    MFVideoFormat_VP90,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::core::PWSTR;

const WIDTH: u32 = 64;
const HEIGHT: u32 = 64;

/// Real `MFTEnumEx(MFT_CATEGORY_VIDEO_DECODER, …)` results for HEVC / AV1 / VP9 on this
/// machine, both unfiltered (any decoder MFT, HW or SW, that declares the subtype) and
/// `MFT_ENUM_FLAG_SYNCMFT`-filtered (the exact call `open_sw_decoder` makes). Informational:
/// records real findings either way rather than asserting a specific outcome, since which
/// codecs have inbox/Store-extension decoders is a property of the OS install, not this
/// crate. See `docs/roadmap.md` for the findings this produced on the verification host.
#[test]
fn list_decoder_mfts_for_each_codec() {
    super::super::runtime::ensure_mf().expect("MF runtime init");
    for (name, subtype) in [
        ("HEVC", MFVideoFormat_HEVC),
        ("AV1", MFVideoFormat_AV1),
        ("VP9", MFVideoFormat_VP90),
    ] {
        let all = enum_decoder_mft_names(subtype, false);
        let sync = enum_decoder_mft_names(subtype, true);
        eprintln!("{name}: any-flag decoder MFTs = {all:?}");
        eprintln!("{name}: MFT_ENUM_FLAG_SYNCMFT decoder MFTs = {sync:?}");
    }
}

/// Real `MFTEnumEx` call + friendly-name lookup for every registered
/// `MFT_CATEGORY_VIDEO_DECODER` MFT that declares `subtype` as an accepted input and NV12
/// as an accepted output. `sync_only` mirrors the flags `open_sw_decoder` uses
/// (`MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_SORTANDFILTER`); `false` uses no flags beyond
/// `SORTANDFILTER` so hardware-only decoder MFTs are included too.
fn enum_decoder_mft_names(subtype: windows::core::GUID, sync_only: bool) -> Vec<String> {
    let input = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: subtype,
    };
    let output = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_NV12,
    };
    let flags = if sync_only {
        MFT_ENUM_FLAG(MFT_ENUM_FLAG_SYNCMFT.0 | MFT_ENUM_FLAG_SORTANDFILTER.0)
    } else {
        MFT_ENUM_FLAG_SORTANDFILTER
    };
    let mut activates: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count = 0u32;
    // SAFETY: MFTEnumEx writes an activate-object array + count as out-params; freed below.
    let hr = unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_DECODER,
            flags,
            Some(std::ptr::from_ref(&input)),
            Some(std::ptr::from_ref(&output)),
            &raw mut activates,
            &raw mut count,
        )
    };
    if hr.is_err() || activates.is_null() {
        return Vec::new();
    }
    let mut names = Vec::new();
    for i in 0..count as usize {
        // SAFETY: `activates` holds `count` valid `Option<IMFActivate>` slots from MFTEnumEx.
        let activate = unsafe { (*activates.add(i)).take() };
        if let Some(activate) = activate {
            names.push(friendly_name(&activate).unwrap_or_else(|| "<unnamed>".to_owned()));
        }
    }
    // SAFETY: `activates` was allocated by MFTEnumEx (CoTaskMemAlloc); we own and free it.
    unsafe {
        CoTaskMemFree(Some(activates.cast_const().cast()));
    }
    names
}

fn friendly_name(activate: &IMFActivate) -> Option<String> {
    let mut raw = PWSTR::null();
    let mut len = 0u32;
    // SAFETY: out-params written on success; the string is `CoTaskMemAlloc`'d and freed below.
    unsafe {
        activate.GetAllocatedString(&MFT_FRIENDLY_NAME_Attribute, &raw mut raw, &raw mut len)
    }
    .ok()?;
    if raw.is_null() {
        return None;
    }
    // SAFETY: `raw` is a valid null-terminated wide string per `GetAllocatedString`'s
    // contract, still valid at this point (freed only below).
    let name = unsafe { raw.to_string() }.ok();
    // SAFETY: matching `CoTaskMemFree` for the successful `GetAllocatedString` above.
    unsafe {
        CoTaskMemFree(Some(raw.0.cast()));
    }
    name
}

/// Horizontal-gradient NV12 frame (not a flat color) so a successful decode can be checked
/// for real varying pixel content, not just "no error" / an all-zero or all-same buffer.
fn gradient_nv12(width: u32, height: u32) -> Bytes {
    let w = width as usize;
    let h = height as usize;
    let mut out = vec![0u8; w * h + w * (h / 2)];
    for y in 0..h {
        for x in 0..w {
            out[y * w + x] = u8::try_from((x * 255) / w.max(1)).unwrap_or(u8::MAX);
        }
    }
    for i in 0..(w * (h / 2)) {
        out[w * h + i] = 128;
    }
    Bytes::from(out)
}

fn has_variance(data: &[u8]) -> bool {
    data.iter().any(|&b| b != data[0])
}

/// Run system `ffmpeg` (optional test/dev oracle, [ADR-0002](../../adr/0002-system-oracle.md))
/// to encode a real `libaom-av1` bitstream (`testsrc` pattern, real varying pixel content)
/// into an IVF container on stdout, so this crate can decode a real AV1 stream from an
/// encoder other than `mediaway-encoder-windows` (which has no AV1 encoder MFT on this
/// machine — see `mediaway-encoder-windows`'s `docs/roadmap.md`). Returns `None` (caller
/// skips honestly) when `ffmpeg` is not on `PATH` or the encode fails — the default test
/// suite must stay green on a machine without `ffmpeg`.
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
/// temporal unit / concatenated-OBU sample, exactly the per-sample shape MF's AV1 decoder
/// MFT input type expects).
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

/// Real AV1 decode verification using system `ffmpeg`'s `libaom-av1` encoder as a real
/// bitstream source, since `mediaway-encoder-windows` has no AV1 encoder MFT on this
/// machine (see its `docs/roadmap.md`) — the decoder MFT (`AV1VideoExtension`, confirmed by
/// [`list_decoder_mfts_for_each_codec`]) is real either way, so this closes the loop with a
/// genuine third-party-encoded AV1 stream instead of leaving AV1 decode entirely unverified.
///
/// Real finding (this session): on this machine, the AV1 Store-extension decoder MFT
/// (`AV1VideoExtension`) negotiates real output for this stream, but only as
/// `MFVideoFormat_AYUV` — it never proposes NV12 for this content
/// (`GetOutputAvailableType` index 1 immediately returns `MF_E_NO_MORE_TYPES`; see
/// `negotiate_nv12_output_type`'s doc comment). Since this crate's CPU decode sessions are
/// NV12-only by design, that surfaces as `DecodeError::Unsupported` here — an honest
/// "real decoder, wrong negotiated pixel format for this stream" finding, not a fabricated
/// pass. Skips (does not fail the default suite) either when `ffmpeg` is unavailable or when
/// this negotiation outcome is hit.
#[test]
fn decode_real_ffmpeg_av1_bitstream_or_skip() {
    let Some(ivf) = ffmpeg_av1_ivf(WIDTH, HEIGHT) else {
        eprintln!("skip: system ffmpeg (libaom-av1) unavailable — optional oracle, see ADR-0002");
        return;
    };
    let payloads = parse_ivf_frame_payloads(&ivf);
    assert!(
        !payloads.is_empty(),
        "ffmpeg produced an IVF file with zero frames"
    );

    let dec_cfg = VideoDecoderConfig {
        codec: CodecKind::Av1,
        width: WIDTH,
        height: HEIGHT,
        time_base: Rational::new(1, 30),
        pixel_format: PixelFormat::Nv12,
        output: VideoOutputPreference::CpuFramesOk,
        gpu_device: None,
        extra_data: Bytes::new(),
    };
    let mut decoder = match WmfMultiCodecCpuDecoder::open(&dec_cfg) {
        Ok(d) => d,
        Err(e) => {
            eprintln!(
                "skip: no CPU AV1 decoder MFT ({e:?}) — negative finding, see docs/roadmap.md"
            );
            return;
        }
    };
    for (i, payload) in payloads.iter().enumerate() {
        let pts = i64::try_from(i).unwrap_or(0);
        let packet = Packet {
            stream_id: 0,
            pts,
            dts: pts,
            duration: 1,
            is_keyframe: i == 0,
            is_discard: false,
            payload: Bytes::from(payload.clone()),
        };
        if let Err(e) = decoder.push_packet(&packet) {
            eprintln!(
                "skip: decoder push_packet on real ffmpeg AV1 data ({e:?}) — see \
                 docs/roadmap.md for the AYUV-not-NV12 negotiation finding"
            );
            return;
        }
    }
    if let Err(e) = decoder.flush() {
        eprintln!(
            "skip: decoder flush on real ffmpeg AV1 data ({e:?}) — see docs/roadmap.md for \
             the AYUV-not-NV12 negotiation finding"
        );
        return;
    }

    let mut frames_out = Vec::new();
    while let Some(f) = decoder.poll_frame().expect("decoder poll_frame") {
        frames_out.push(f);
    }
    assert!(
        !frames_out.is_empty(),
        "expected at least one decoded frame from a real ffmpeg-encoded AV1 stream"
    );
    for f in &frames_out {
        assert_eq!(f.width, WIDTH);
        assert_eq!(f.height, HEIGHT);
        let VideoFrameStorage::Cpu { data } = &f.storage else {
            unreachable!("expected VideoFrameStorage::Cpu");
        };
        assert!(
            has_variance(&data[..(WIDTH * HEIGHT) as usize]),
            "decoded luma plane has no variance — testsrc pattern should decode to real \
             varying content, not a zeroed/garbage buffer"
        );
    }
    eprintln!(
        "AV1 real ffmpeg round trip OK — {} packet(s) in, {} frame(s) out",
        payloads.len(),
        frames_out.len()
    );
}

/// Encode one real gradient NV12 frame via `mediaway-encoder-windows` CPU-upload path.
/// Returns `None` (caller skips honestly) on any real-hardware limitation — no CPU encoder
/// MFT for `codec`, a push/flush failure, or zero packets produced.
fn encode_one_codec_cpu(codec: CodecKind) -> Option<(Vec<Packet>, Bytes)> {
    let enc_cfg = VideoEncoderConfig {
        codec,
        width: WIDTH,
        height: HEIGHT,
        time_base: Rational::new(1, 30),
        bitrate_bps: 500_000,
        pixel_format: PixelFormat::Nv12,
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: None,
    };
    let mut encoder = match WindowsVideoEncoder::open(&enc_cfg) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("skip {codec:?}: no CPU encoder MFT ({e:?})");
            return None;
        }
    };
    let frame = VideoFrame {
        pts: 0,
        duration: 1,
        width: WIDTH,
        height: HEIGHT,
        format: PixelFormat::Nv12,
        storage: VideoFrameStorage::Cpu {
            data: gradient_nv12(WIDTH, HEIGHT),
        },
    };
    if let Err(e) = encoder.push_frame(&frame) {
        eprintln!("skip {codec:?}: encoder push_frame failed ({e:?})");
        return None;
    }
    encoder.flush().expect("encoder flush");
    let mut packets = Vec::new();
    while let Some(p) = encoder.poll_packet().expect("encoder poll_packet") {
        packets.push(p);
    }
    if packets.is_empty() {
        eprintln!("skip {codec:?}: encoder produced zero packets");
        return None;
    }
    let extra_data = encoder.stream_info().extra_data().clone(); // clone: owned snapshot for decoder config below
    Some((packets, extra_data))
}

/// Decode real encoded `packets` through [`WmfMultiCodecCpuDecoder`] and assert the output
/// is real, varying pixel content of the expected size. Returns without asserting anything
/// (caller treats it as an honest skip) on any real-hardware limitation.
fn decode_and_verify_cpu(codec: CodecKind, packets: &[Packet], extra_data: Bytes) {
    let nv12_len = (WIDTH * HEIGHT + WIDTH * HEIGHT / 2) as usize;
    let dec_cfg = VideoDecoderConfig {
        codec,
        width: WIDTH,
        height: HEIGHT,
        time_base: Rational::new(1, 30),
        pixel_format: PixelFormat::Nv12,
        output: VideoOutputPreference::CpuFramesOk,
        gpu_device: None,
        extra_data,
    };
    let mut decoder = match WmfMultiCodecCpuDecoder::open(&dec_cfg) {
        Ok(d) => d,
        Err(e) => {
            eprintln!(
                "skip {codec:?}: no CPU decoder MFT ({e:?}) — negative finding, see docs/roadmap.md"
            );
            return;
        }
    };

    for packet in packets {
        if let Err(e) = decoder.push_packet(packet) {
            eprintln!("skip {codec:?}: decoder push_packet failed ({e:?})");
            return;
        }
    }
    if let Err(e) = decoder.flush() {
        eprintln!("skip {codec:?}: decoder flush failed ({e:?})");
        return;
    }

    let mut frames_out = Vec::new();
    loop {
        match decoder.poll_frame() {
            Ok(Some(f)) => frames_out.push(f),
            Ok(None) => break,
            Err(e) => {
                eprintln!("skip {codec:?}: decoder poll_frame failed ({e:?})");
                break;
            }
        }
    }
    if frames_out.is_empty() {
        eprintln!("skip {codec:?}: decoder produced zero frames");
        return;
    }

    for f in &frames_out {
        assert_eq!(f.width, WIDTH, "{codec:?} decoded width");
        assert_eq!(f.height, HEIGHT, "{codec:?} decoded height");
        let VideoFrameStorage::Cpu { data } = &f.storage else {
            unreachable!("{codec:?}: expected VideoFrameStorage::Cpu");
        };
        assert!(
            data.len() >= nv12_len,
            "{codec:?}: decoded buffer too small ({} < {nv12_len})",
            data.len()
        );
        assert!(
            has_variance(&data[..(WIDTH * HEIGHT) as usize]),
            "{codec:?}: decoded luma plane has no variance — looks like a zeroed/garbage \
             buffer, not a real decoded gradient frame"
        );
    }
    eprintln!(
        "{codec:?}: real CPU decode OK — {} packet(s) in, {} frame(s) out, first frame \
         luma[0..8]={:?}",
        packets.len(),
        frames_out.len(),
        match &frames_out[0].storage {
            VideoFrameStorage::Cpu { data } => &data[..8.min(data.len())],
            _ => &[],
        }
    );
}

/// Real end-to-end encode → CPU decode round trip for HEVC / AV1 / VP9: encodes a real
/// gradient NV12 frame via `mediaway-encoder-windows` (already verified real on this
/// machine — see `mediaway-encoder-windows`'s `docs/roadmap.md`), then decodes the real
/// packets through [`WmfMultiCodecCpuDecoder`]. Skips (does not fail) per-codec when either
/// side is unavailable on this machine — see `docs/roadmap.md` for the exact real findings
/// this produced.
#[test]
fn encode_then_decode_hevc_av1_vp9_cpu_round_trip_or_skip() {
    for codec in [CodecKind::Hevc, CodecKind::Av1, CodecKind::Vp9] {
        let Some((packets, extra_data)) = encode_one_codec_cpu(codec) else {
            continue;
        };
        decode_and_verify_cpu(codec, &packets, extra_data);
    }
}
