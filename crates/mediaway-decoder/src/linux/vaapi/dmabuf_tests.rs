#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test modules may unwrap / panic-on-unexpected-variant"
)]

use std::fs::File;
use std::os::fd::OwnedFd;

use cros_libva::{DrmPrimeSurfaceDescriptor, DrmPrimeSurfaceDescriptorLayer};
use mediaway_common::GpuBufferHandle;

use super::build_from_prime;

/// A real, harmless fd (`/dev/null`) — this module's validation logic is pure data (object/
/// layer/plane-count checks), so any open fd stands in for a real VA-API-exported one; no
/// VA-API device is needed (mirrors `dpb_tests.rs`'s own no-device-needed pattern).
fn dev_null_fd() -> OwnedFd {
    File::open("/dev/null")
        .expect("/dev/null always exists on Linux")
        .into()
}

fn descriptor(
    object_count: usize,
    layers: Vec<DrmPrimeSurfaceDescriptorLayer>,
) -> DrmPrimeSurfaceDescriptor {
    DrmPrimeSurfaceDescriptor {
        fourcc: 0x3231_564e, // DRM_FORMAT_NV12 ('N''V''1''2' little-endian)
        width: 1920,
        height: 1080,
        objects: (0..object_count)
            .map(|i| cros_libva::DrmPrimeSurfaceDescriptorObject {
                fd: dev_null_fd(),
                size: 1024,
                drm_format_modifier: i as u64,
            })
            .collect(),
        layers,
    }
}

fn nv12_layer(object_index: [u8; 4], num_planes: u32) -> DrmPrimeSurfaceDescriptorLayer {
    DrmPrimeSurfaceDescriptorLayer {
        drm_format: 0x3231_564e,
        num_planes,
        object_index,
        offset: [0, 1920 * 1080, 0, 0],
        pitch: [1920, 1920, 0, 0],
    }
}

#[test]
fn builds_handle_from_single_object_two_plane_nv12() {
    let desc = descriptor(1, vec![nv12_layer([0, 0, 0, 0], 2)]);
    let (handle, fds) = build_from_prime(desc).unwrap();
    let GpuBufferHandle::DmaBuf(d) = handle else {
        panic!("expected DmaBuf variant");
    };
    assert_eq!(d.plane_count, 2);
    assert_eq!(d.width, 1920);
    assert_eq!(d.height, 1080);
    assert!(d.fd1.is_none());
    assert!(fds.fd1.is_none());
}

#[test]
fn builds_handle_from_two_objects() {
    let desc = descriptor(2, vec![nv12_layer([0, 1, 0, 0], 2)]);
    let (handle, fds) = build_from_prime(desc).unwrap();
    let GpuBufferHandle::DmaBuf(d) = handle else {
        panic!("expected DmaBuf variant");
    };
    assert!(d.fd1.is_some());
    assert!(fds.fd1.is_some());
}

#[test]
fn rejects_zero_objects() {
    let desc = descriptor(0, vec![nv12_layer([0, 0, 0, 0], 2)]);
    assert!(build_from_prime(desc).is_err());
}

#[test]
fn rejects_too_many_objects() {
    let desc = descriptor(3, vec![nv12_layer([0, 1, 2, 0], 2)]);
    assert!(build_from_prime(desc).is_err());
}

#[test]
fn rejects_wrong_layer_count() {
    let desc = descriptor(1, vec![]);
    assert!(build_from_prime(desc).is_err());

    let desc = descriptor(
        1,
        vec![nv12_layer([0, 0, 0, 0], 2), nv12_layer([0, 0, 0, 0], 2)],
    );
    assert!(build_from_prime(desc).is_err());
}

#[test]
fn rejects_zero_planes() {
    let desc = descriptor(1, vec![nv12_layer([0, 0, 0, 0], 0)]);
    assert!(build_from_prime(desc).is_err());
}

#[test]
fn rejects_too_many_planes() {
    let desc = descriptor(1, vec![nv12_layer([0, 0, 0, 0], 3)]);
    assert!(build_from_prime(desc).is_err());
}
