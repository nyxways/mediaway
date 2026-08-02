#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

#[test]
fn rational_equality_when_same_fraction() {
    assert_eq!(Rational::new(1, 60), Rational::new(1, 60));
    assert_ne!(Rational::new(1, 60), Rational::new(1001, 60_000));
}

#[test]
fn packet_bytes_clone_shares_payload() {
    let payload = Bytes::from_static(&[0x65, 0x88]);
    let a = Packet {
        stream_id: 0,
        pts: 0,
        dts: 0,
        duration: 1,
        is_keyframe: true,
        is_discard: false,
        payload,
    };
    let b = a.clone();
    assert_eq!(a.payload, b.payload);
}

#[test]
fn stream_info_audio_sample_rate_and_channels() {
    let info = StreamInfo::Audio {
        id: 1,
        codec: CodecKind::Aac,
        time_base: Rational::new(1, 48_000),
        extra_data: Bytes::new(),
        sample_rate: 48_000,
        channels: 2,
    };
    assert_eq!(info.sample_rate(), Some(48_000));
    assert_eq!(info.channels(), Some(2));
    assert_eq!(info.geometry(), None);
}

#[test]
fn stream_info_video_has_no_sample_rate_or_channels() {
    let info = StreamInfo::Video {
        id: 0,
        codec: CodecKind::H264,
        time_base: Rational::new(1, 30),
        geometry: VideoGeometry {
            width: 1920,
            height: 1080,
        },
        extra_data: Bytes::new(),
    };
    assert_eq!(info.sample_rate(), None);
    assert_eq!(info.channels(), None);
}

#[test]
fn stream_info_with_id_preserves_audio_fields() {
    let info = StreamInfo::Audio {
        id: 1,
        codec: CodecKind::Opus,
        time_base: Rational::new(1, 48_000),
        extra_data: Bytes::new(),
        sample_rate: 48_000,
        channels: 1,
    }
    .with_id(7);
    assert_eq!(info.id(), 7);
    assert_eq!(info.sample_rate(), Some(48_000));
    assert_eq!(info.channels(), Some(1));
}

#[test]
fn video_frame_gpu_handle_is_copy() {
    let h = GpuBufferHandle::DirectX11 {
        texture: NativeHandle::new(0x1000).unwrap(),
        subresource: 0,
    };
    let frame = VideoFrame {
        pts: 0,
        duration: 1,
        width: 16,
        height: 16,
        format: PixelFormat::Nv12,
        storage: VideoFrameStorage::Gpu(h),
    };
    assert_eq!(frame.width, 16);
    let VideoFrameStorage::Gpu(GpuBufferHandle::DirectX11 { texture, .. }) = frame.storage else {
        unreachable!("constructed as DirectX11 above")
    };
    assert_eq!(texture, NativeHandle::new(0x1000).unwrap());
}
