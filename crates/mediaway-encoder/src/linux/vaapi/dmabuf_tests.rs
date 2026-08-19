#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use std::fs::File;
use std::os::fd::{AsRawFd, OwnedFd};

use cros_libva::ExternalBufferDescriptor;
use mediaway_common::{DmaBufDescriptor, DmaBufPlane, NativeHandle};

use super::{DmaBufImportDescriptor, dup_from_native};

/// `DRM_FORMAT_NV12` / `VA_FOURCC_NV12` ('N''V''1''2' little-endian) — numerically identical,
/// per this ADR's (unconfirmed on real hardware) Open questions #2.
const NV12_FOURCC: u32 = 0x3231_564e;

/// A real, harmless fd (`/dev/null`) — this module's pure-data assembly (`va_surface_attribute`)
/// never reads through the fd's contents, and `dup_from_native` needs a genuinely open fd to
/// duplicate; no VA-API device is needed for either (mirrors the decoder companion's
/// `dmabuf_tests.rs` no-device-needed pattern).
fn dev_null_fd() -> OwnedFd {
    File::open("/dev/null")
        .expect("/dev/null always exists on Linux")
        .into()
}

fn nv12_planes() -> [DmaBufPlane; 2] {
    [
        DmaBufPlane {
            object_index: 0,
            offset: 0,
            pitch: 64,
        },
        DmaBufPlane {
            object_index: 0,
            offset: 64 * 64,
            pitch: 64,
        },
    ]
}

#[test]
fn va_surface_attribute_assembles_single_object_nv12_descriptor() {
    let mut desc = DmaBufImportDescriptor {
        fourcc: NV12_FOURCC,
        width: 64,
        height: 64,
        modifier: 7,
        fd0: None,
        fd1: None,
        planes: nv12_planes(),
        plane_count: 2,
    };
    let attr = desc.va_surface_attribute();

    assert_eq!(attr.fourcc, NV12_FOURCC);
    assert_eq!(attr.width, 64);
    assert_eq!(attr.height, 64);
    assert_eq!(attr.num_objects, 1);
    assert_eq!(attr.num_layers, 1);
    assert_eq!(attr.layers[0].drm_format, NV12_FOURCC);
    assert_eq!(attr.layers[0].num_planes, 2);
    assert_eq!(attr.layers[0].offset[1], 64 * 64);
    assert_eq!(attr.layers[0].pitch[0], 64);
}

#[test]
fn va_surface_attribute_reports_two_objects_when_fd1_present() {
    let mut desc = DmaBufImportDescriptor {
        fourcc: NV12_FOURCC,
        width: 64,
        height: 64,
        modifier: 0,
        fd0: Some(dev_null_fd()),
        fd1: Some(dev_null_fd()),
        planes: nv12_planes(),
        plane_count: 2,
    };
    let attr = desc.va_surface_attribute();
    assert_eq!(attr.num_objects, 2);
    // `desc.fd0`/`fd1` are read via `as_raw_fd()`, not consumed — both stay `Some` afterward.
    assert!(desc.fd0.is_some());
    assert!(desc.fd1.is_some());
}

#[test]
fn dup_from_native_duplicates_a_real_fd() {
    let fd = dev_null_fd();
    let raw = fd.as_raw_fd();
    let handle = NativeHandle::new(usize::try_from(raw).unwrap() + 1).unwrap();

    let duped = dup_from_native(handle).unwrap();
    assert_ne!(
        duped.as_raw_fd(),
        raw,
        "dup must return a distinct fd number"
    );
    drop(fd);
}

#[test]
fn dup_from_native_rejects_bits_that_overflow_raw_fd() {
    // `usize::MAX` decodes (after the `-1` offset) to a value far beyond any valid `RawFd`
    // (`i32`) — must be rejected as `InvalidInput`, never passed to a real syscall.
    let handle = NativeHandle::new(usize::MAX).unwrap();
    assert!(dup_from_native(handle).is_err());
}

#[test]
fn dma_buf_descriptor_fd0_round_trips_through_native_handle_convention() {
    // Confirms this crate reads the shared `mediaway_common::DmaBufDescriptor::fd0` convention
    // (`+1` offset, see that field's doc) the same way the decoder companion writes it.
    let fd = dev_null_fd();
    let bits = usize::try_from(fd.as_raw_fd()).unwrap() + 1;
    let descriptor = DmaBufDescriptor {
        fd0: NativeHandle::new(bits).unwrap(),
        fd1: None,
        fourcc: NV12_FOURCC,
        modifier: 0,
        width: 64,
        height: 64,
        planes: nv12_planes(),
        plane_count: 2,
    };
    assert_eq!(descriptor.fd0.get(), bits);
}
