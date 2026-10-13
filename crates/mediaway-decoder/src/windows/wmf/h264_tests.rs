#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "unit tests"
)]

use super::*;
use mediaway_common::{CodecKind, ColorRange, Rational};
use mediaway_encoder::windows::WindowsVideoEncoder;
use mediaway_encoder::{VideoEncoder, VideoEncoderConfig, VideoInputPreference};

/// A size that is not a multiple of 16, as a screen region usually is (Recoway's 724 x 274).
const WIDTH: u32 = 724;
const HEIGHT: u32 = 274;
const FRAMES: i64 = 20;

/// NV12 with a gradient that moves each frame, so every frame is a real picture to decode.
fn moving_nv12(n: i64) -> Bytes {
    let (w, h) = (WIDTH as usize, HEIGHT as usize);
    let shift = usize::try_from(n).unwrap() * 7;
    let mut out = vec![128u8; w * h + w * (h / 2)];
    for y in 0..h {
        for x in 0..w {
            out[y * w + x] = u8::try_from(((x + shift) * 255 / w) % 256).unwrap();
        }
    }
    Bytes::from(out)
}

/// Real finding (2026-10-13): the inbox H.264 decoder signals `MF_E_TRANSFORM_STREAM_CHANGE` when
/// the first frame of a stream whose size is not a multiple of 16 is ready (it pads the output to
/// whole macroblocks). The CPU path used to set the input type again and restart streaming there,
/// which discarded every buffered frame: such a stream decoded to zero frames, flush included.
/// Encodes `FRAMES` frames at 724 x 274 with the CPU encoder and decodes them all back.
#[test]
fn a_size_that_is_not_a_multiple_of_16_decodes_every_frame_on_the_cpu() {
    let mut encoder = match WindowsVideoEncoder::open(&VideoEncoderConfig {
        codec: CodecKind::H264,
        width: WIDTH,
        height: HEIGHT,
        time_base: Rational::new(1, 30),
        bitrate_bps: 1_000_000,
        pixel_format: PixelFormat::Nv12,
        color_range: ColorRange::Video,
        input: VideoInputPreference::CpuUploadOk,
        gpu_device: None,
        gop_size: 30,
        rate_control: None,
        intra_refresh_period: None,
    }) {
        Ok(encoder) => encoder,
        Err(e) => {
            eprintln!("skip: no CPU H.264 encoder MFT ({e:?})");
            return;
        }
    };
    let mut packets = Vec::new();
    for n in 0..FRAMES {
        encoder
            .push_frame(&VideoFrame {
                pts: n,
                duration: 1,
                width: WIDTH,
                height: HEIGHT,
                format: PixelFormat::Nv12,
                storage: VideoFrameStorage::Cpu {
                    data: moving_nv12(n),
                },
            })
            .expect("encode");
        while let Some(p) = encoder.poll_packet().expect("poll_packet") {
            packets.push(p);
        }
    }
    encoder.flush().expect("encoder flush");
    while let Some(p) = encoder.poll_packet().expect("poll_packet") {
        packets.push(p);
    }
    let extra_data = encoder.stream_info().extra_data().clone(); // clone: owned snapshot for the decoder config

    let mut decoder = WmfH264Decoder::open(&VideoDecoderConfig {
        codec: CodecKind::H264,
        width: WIDTH,
        height: HEIGHT,
        time_base: Rational::new(1, 30),
        pixel_format: PixelFormat::Nv12,
        output: VideoOutputPreference::CpuFramesOk,
        gpu_device: None,
        extra_data,
    })
    .expect("the CPU H.264 decoder");
    let mut frames = Vec::new();
    for packet in &packets {
        decoder.push_packet(packet).expect("push_packet");
        while let Some(f) = decoder.poll_frame().expect("poll_frame") {
            frames.push(f);
        }
    }
    decoder.flush().expect("flush");
    while let Some(f) = decoder.poll_frame().expect("poll_frame") {
        frames.push(f);
    }
    let mut sent: Vec<i64> = packets.iter().map(|p| p.pts).collect();
    sent.sort_unstable();
    assert_eq!(
        sent.len(),
        usize::try_from(FRAMES).unwrap(),
        "one packet a frame"
    );
    assert_eq!(
        frames.iter().map(|f| f.pts).collect::<Vec<_>>(),
        sent,
        "every frame, in order"
    );
    for f in &frames {
        // The decoder's own padded size: at least the stream's, whole macroblocks.
        assert!(
            f.width >= WIDTH && f.height >= HEIGHT,
            "{}x{}",
            f.width,
            f.height
        );
        let VideoFrameStorage::Cpu { data } = &f.storage else {
            unreachable!("CPU output");
        };
        assert!(data.len() >= (f.width * f.height * 3 / 2) as usize);
    }
}

/// The bytes of an `IMFSample`'s single buffer.
fn sample_bytes(sample: &IMFSample) -> Vec<u8> {
    use windows::Win32::Media::MediaFoundation::IMFMediaBuffer;
    // SAFETY: a lock/unlock pair on a buffer this test owns; the bytes are copied before unlock.
    unsafe {
        let buffer: IMFMediaBuffer = sample.ConvertToContiguousBuffer().expect("one buffer");
        let (mut ptr, mut max, mut len) = (std::ptr::null_mut(), 0u32, 0u32);
        buffer
            .Lock(&raw mut ptr, Some(&raw mut max), Some(&raw mut len))
            .expect("lock");
        let bytes = std::slice::from_raw_parts(ptr, len as usize).to_vec();
        buffer.Unlock().expect("unlock");
        bytes
    }
}

/// Found 2026-10-14 with a libx264 file: an AVCC packet whose first NAL is 256 to 511 bytes long
/// begins `00 00 01 xx`, which the start-code probe took for an Annex-B start code, so the packet
/// went to the MFT with its length prefixes in it and the MFT dropped the frame (26 of 218 frames
/// of a screen recording never came out).
#[test]
fn a_packet_whose_first_nal_is_256_to_511_bytes_is_converted_from_avcc() {
    ensure_mf_runtime().expect("Media Foundation");
    for len in [255usize, 256, 300, 511, 512] {
        let nal: Vec<u8> = std::iter::once(0x41u8)
            .chain(std::iter::repeat_n(0x9a, len - 1))
            .collect();
        let mut payload = u32::try_from(len).unwrap().to_be_bytes().to_vec();
        payload.extend_from_slice(&nal);
        let packet = Packet {
            stream_id: 0,
            pts: 0,
            dts: 0,
            duration: 1,
            is_keyframe: false,
            is_discard: false,
            payload: Bytes::from(payload),
        };
        let sample = packet_to_sample(&packet, 1, 30, NalFraming::Avc(4)).expect("sample");
        let bytes = sample_bytes(&sample);
        let mut annex_b = vec![0u8, 0, 0, 1];
        annex_b.extend_from_slice(&nal);
        assert_eq!(bytes, annex_b, "{len}-byte NAL");
    }
}

#[test]
fn a_packet_that_is_already_annex_b_goes_through_untouched() {
    ensure_mf_runtime().expect("Media Foundation");
    let payload = vec![0u8, 0, 0, 1, 0x65, 0x88, 0x84, 0x21, 0x00];
    let packet = Packet {
        stream_id: 0,
        pts: 0,
        dts: 0,
        duration: 1,
        is_keyframe: true,
        is_discard: false,
        payload: Bytes::from(payload.clone()),
    };
    let sample = packet_to_sample(&packet, 1, 30, NalFraming::Avc(4)).expect("sample");
    assert_eq!(sample_bytes(&sample), payload);
}

#[test]
fn a_frame_stamped_with_a_time_no_packet_had_gets_the_earliest_waiting_time() {
    let mut waiting = InFlight::default();
    for pts in [0, 10, 20, 30, 40] {
        waiting.add(pts);
    }
    assert_eq!(waiting.claim(0), 0);
    assert_eq!(waiting.claim(10), 10);
    // The MFT stamped the next frame 0 again (the drain's last IDR frame, 2026-10-14).
    assert_eq!(waiting.claim(0), 20);
    // Times that were packets' keep their own, whatever is still waiting before them.
    assert_eq!(waiting.claim(40), 40);
    assert_eq!(waiting.claim(30), 30);
    // Nothing waits: the stamp stands.
    assert_eq!(waiting.claim(0), 0);
}

#[test]
fn two_packets_with_one_time_are_both_claimed_by_it() {
    let mut waiting = InFlight::default();
    waiting.add(5);
    waiting.add(5);
    waiting.add(9);
    assert_eq!(waiting.claim(5), 5);
    assert_eq!(waiting.claim(5), 5);
    assert_eq!(waiting.claim(5), 9, "a third frame at 5 had no packet");
}
