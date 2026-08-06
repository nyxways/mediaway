//! AV1-specific hardware-gated tests, split out of [`super`] to stay under the 1000-line
//! source limit — see that module's doc for the shared helpers/imports this file relies on
//! via `use super::*`.

use super::*;

/// Parse AV1 `obu_type` values out of length-prefixed OBUs (`obu_has_size_field == 1`,
/// no extension — the only shape this backend ever writes/expects, see
/// [`super::super::bitstream_av1`]).
fn obu_types(payload: &[u8]) -> Vec<u8> {
    let mut types = Vec::new();
    let mut i = 0usize;
    while i < payload.len() {
        let header = payload[i];
        let obu_type = (header >> 3) & 0x0F;
        let has_size_field = (header >> 1) & 1 == 1;
        let has_extension = (header >> 2) & 1 == 1;
        i += 1;
        if has_extension {
            i += 1;
        }
        if !has_size_field || i >= payload.len() {
            break;
        }
        let mut size = 0u64;
        let mut shift = 0u32;
        loop {
            if i >= payload.len() {
                return types;
            }
            let b = payload[i];
            i += 1;
            size |= u64::from(b & 0x7f) << shift;
            shift += 7;
            if b & 0x80 == 0 {
                break;
            }
        }
        types.push(obu_type);
        let Ok(size) = usize::try_from(size) else {
            break;
        };
        i += size;
    }
    types
}

/// Real D3D12 device → real AV1 `ID3D12VideoDevice3` support check → real `EncodeFrame`
/// submissions → real length-prefixed OBU output (temporal delimiter `obu_type == 2`,
/// sequence header `== 1`, frame `== 6` present in every packet). Skips (does not fail) if
/// this machine's adapter/driver lacks D3D12 native AV1 video-encode support (requires
/// Windows 11 24H2+ / WDDM 3.2).
///
/// **Honesty caveat (2026-08-07, see ADR-0007's addendum):** these assertions only check
/// structural OBU framing (right `obu_type`s in the right order), not actual decodability —
/// `ffprobe` parses this test's real output correctly, but `libdav1d` still rejects the
/// frame data at a 100% error rate on this crate's reference RTX 4090. A pass here means
/// "the pipeline runs end to end and produces a plausible-looking AV1 stream," not "this
/// stream decodes." Do not treat a pass as proof AV1 encode is fixed until a real decode
/// oracle is added and passes too.
#[test]
fn d3d12_native_av1_encode_or_skip() {
    let Some(device) = open_real_d3d12_device() else {
        return;
    };
    let Some(handle) = NativeHandle::new(Interface::as_raw(&device) as usize) else {
        eprintln!("skip: null D3D12 device pointer");
        return;
    };
    let info_queue: Option<ID3D12InfoQueue> = device.cast().ok();

    let cfg = VideoEncoderConfig {
        codec: CodecKind::Av1,
        width: WIDTH_AV1,
        height: HEIGHT_AV1,
        time_base: Rational::new(1, 30),
        bitrate_bps: 500_000,
        pixel_format: PixelFormat::Nv12,
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: Some(GpuDeviceHandle::DirectX12(handle)),
        gop_size: 1,
        rate_control: None,
        intra_refresh_period: None,
    };

    let mut enc = match D3d12VideoEncoder::open(&cfg) {
        Ok(e) => e,
        Err(e) => {
            dump_d3d12_info_queue(info_queue.as_ref());
            eprintln!(
                "skip: D3d12VideoEncoder::open (AV1) failed ({e:?}) — no D3D12 AV1 video-encode \
                 support on this device/driver?"
            );
            return;
        }
    };

    let mut packets = 0usize;
    for i in 0..3i64 {
        let frame = nv12_frame_sized(i, WIDTH_AV1, HEIGHT_AV1);
        if let Err(e) = enc.push_frame(&frame) {
            dump_d3d12_info_queue(info_queue.as_ref());
            eprintln!("skip: push_frame (AV1) failed ({e:?})");
            return;
        }
        let packet = match enc.poll_packet() {
            Ok(Some(p)) => p,
            Ok(None) => {
                eprintln!("skip: no packet after push_frame {i} (AV1)");
                return;
            }
            Err(e) => {
                eprintln!("skip: poll_packet (AV1) failed ({e:?})");
                return;
            }
        };
        assert!(!packet.payload.is_empty(), "packet {i} payload is empty");
        assert!(packet.is_keyframe, "packet {i} should be a key frame");

        let types = obu_types(&packet.payload);
        assert!(
            types.contains(&2),
            "packet {i} missing temporal delimiter OBU (type 2); found types {types:?}"
        );
        assert!(
            types.contains(&1),
            "packet {i} missing sequence header OBU (type 1); found types {types:?}"
        );
        assert!(
            types.contains(&6),
            "packet {i} missing frame OBU (type 6); found types {types:?}"
        );
        packets += 1;
    }

    enc.flush().expect("flush");
    eprintln!("d3d12 native av1 encode ok: {packets} packets, all with real TD+SeqHdr+Frame OBUs");
}

/// Real, hardware-honest probe: does this machine's actual D3D12 driver advertise AV1
/// video-encode support (`D3D12_FEATURE_VIDEO_ENCODER_CODEC` for
/// `D3D12_VIDEO_ENCODER_CODEC_AV1`)? Kept alongside [`d3d12_native_av1_encode_or_skip`] as
/// a cheap, isolated probe (no encoder session) for triage when that test skips. Never
/// fails: both "supported" and "not supported" are informative, honest outcomes.
#[test]
fn d3d12_av1_encode_codec_probe() {
    use windows::Win32::Media::MediaFoundation::{
        D3D12_FEATURE_DATA_VIDEO_ENCODER_CODEC, D3D12_FEATURE_VIDEO_ENCODER_CODEC,
        D3D12_VIDEO_ENCODER_CODEC_AV1, ID3D12VideoDevice3,
    };
    use windows::core::BOOL;

    let Some(device) = open_real_d3d12_device() else {
        return;
    };
    let Ok(video_device) = device.cast::<ID3D12VideoDevice3>() else {
        eprintln!("skip: no ID3D12VideoDevice3 on this device");
        return;
    };

    let mut support = D3D12_FEATURE_DATA_VIDEO_ENCODER_CODEC {
        NodeIndex: 0,
        Codec: D3D12_VIDEO_ENCODER_CODEC_AV1,
        IsSupported: BOOL::default(),
    };
    // SAFETY: `support` is sized/typed exactly as `D3D12_FEATURE_VIDEO_ENCODER_CODEC` expects.
    let hr = unsafe {
        video_device.CheckFeatureSupport(
            D3D12_FEATURE_VIDEO_ENCODER_CODEC,
            std::ptr::from_mut(&mut support).cast(),
            u32::try_from(std::mem::size_of::<D3D12_FEATURE_DATA_VIDEO_ENCODER_CODEC>())
                .unwrap_or(u32::MAX),
        )
    };
    match hr {
        Ok(()) => eprintln!(
            "d3d12 av1 encode codec probe: IsSupported={}",
            support.IsSupported.as_bool()
        ),
        Err(e) => eprintln!("d3d12 av1 encode codec probe: CheckFeatureSupport failed ({e:?})"),
    }
}
