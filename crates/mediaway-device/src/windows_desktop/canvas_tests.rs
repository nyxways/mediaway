#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr,
    reason = "unit tests"
)]

use super::{Canvas, CanvasPlacement};
use crate::windows::{GpuDevice, GpuDeviceOptions};
use mediaway_common::{GpuBufferHandle, GpuDeviceHandle, VideoFrameStorage};

#[test]
fn a_smaller_picture_is_centered_on_even_offsets() {
    let p = CanvasPlacement::centered((1920, 1080), (801, 600));
    assert_eq!((p.x, p.y, p.width, p.height), (558, 240, 801, 600));
    assert_eq!((p.source_x, p.source_y), (0, 0));
    let odd = CanvasPlacement::centered((1920, 1080), (1000, 598));
    assert_eq!((odd.x % 2, odd.y % 2), (0, 0));
}

#[test]
fn a_larger_picture_is_cut_around_its_center() {
    let p = CanvasPlacement::centered((1920, 1080), (2560, 1000));
    assert_eq!((p.x, p.width, p.source_x), (0, 1920, 320));
    assert_eq!((p.y, p.height, p.source_y), (40, 1000, 0));
    let same = CanvasPlacement::centered((1920, 1080), (1920, 1080));
    assert_eq!(
        same,
        CanvasPlacement {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            source_x: 0,
            source_y: 0
        }
    );
}

#[test]
fn covering_is_containment_on_the_canvas() {
    let big = CanvasPlacement::centered((100, 100), (60, 60));
    let small = CanvasPlacement::centered((100, 100), (20, 40));
    assert!(big.covers(&small));
    assert!(!small.covers(&big));
    assert!(big.covers(&big));
}

fn device() -> Option<GpuDevice> {
    GpuDevice::create(GpuDeviceOptions::default()).ok()
}

fn handle(device: &GpuDevice) -> mediaway_common::NativeHandle {
    let GpuDeviceHandle::DirectX11(h) = device.handle() else {
        panic!("a D3D11 device");
    };
    h
}

/// A `w` x `h` BGRA picture of one color.
fn solid(w: u32, h: u32, bgra: [u8; 4]) -> Vec<u8> {
    bgra.repeat((w * h) as usize)
}

/// The BGRA pixel at `x`, `y` of a canvas `width` wide.
fn at(pixels: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * width + x) * 4) as usize;
    pixels[i..i + 4].try_into().unwrap()
}

const RED: [u8; 4] = [0, 0, 255, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLACK: [u8; 4] = [0, 0, 0, 255];

#[test]
fn pictures_of_changing_size_land_centered_and_leave_no_trace() {
    let _lock = super::super::HARDWARE_TEST_LOCK.lock();
    let Some(device) = device() else {
        eprintln!("skip: no D3D11 device");
        return;
    };
    let mut canvas = Canvas::new(handle(&device), 320, 200).unwrap();
    // A large picture, then (four slots later, in the same slot) a smaller one: the slot must be
    // black again outside the smaller one.
    for _ in 0..4 {
        canvas
            .place_pixels(&solid(300, 180, RED), 300, 180)
            .unwrap();
    }
    let placed = canvas
        .place_pixels(&solid(100, 60, GREEN), 100, 60)
        .unwrap();
    assert_eq!((placed.placed.x, placed.placed.y), (110, 70));
    assert_eq!((placed.frame.width, placed.frame.height), (320, 200));
    assert!(matches!(
        placed.frame.storage,
        VideoFrameStorage::Gpu(GpuBufferHandle::DirectX11 { .. })
    ));
    let pixels = canvas.read_last().unwrap().unwrap();
    assert_eq!(at(&pixels, 320, 110, 70), GREEN);
    assert_eq!(at(&pixels, 320, 209, 129), GREEN);
    assert_eq!(
        at(&pixels, 320, 109, 70),
        BLACK,
        "left of it, where red was"
    );
    assert_eq!(at(&pixels, 320, 210, 129), BLACK);
    assert_eq!(at(&pixels, 320, 160, 69), BLACK);
    assert_eq!(at(&pixels, 320, 0, 0), BLACK);
}

#[test]
fn a_texture_is_copied_and_a_wider_one_is_cut_to_the_canvas() {
    let _lock = super::super::HARDWARE_TEST_LOCK.lock();
    let Some(device) = device() else {
        eprintln!("skip: no D3D11 device");
        return;
    };
    // The source: a 400 x 100 canvas holding a picture red on its left half, green on its right.
    let mut source = Canvas::new(handle(&device), 400, 100).unwrap();
    let mut picture = Vec::new();
    for _ in 0..100 {
        picture.extend(RED.repeat(200));
        picture.extend(GREEN.repeat(200));
    }
    let src = source.place_pixels(&picture, 400, 100).unwrap();
    let VideoFrameStorage::Gpu(GpuBufferHandle::DirectX11 { texture, .. }) = src.frame.storage
    else {
        panic!("a texture");
    };
    let mut canvas = Canvas::new(handle(&device), 300, 200).unwrap();
    let placed = canvas.place_texture(texture, (400, 100)).unwrap();
    assert_eq!(
        placed.placed,
        CanvasPlacement {
            x: 0,
            y: 50,
            width: 300,
            height: 100,
            source_x: 50,
            source_y: 0
        }
    );
    let pixels = canvas.read_last().unwrap().unwrap();
    assert_eq!(at(&pixels, 300, 0, 50), RED);
    assert_eq!(at(&pixels, 300, 149, 149), RED, "source column 199");
    assert_eq!(at(&pixels, 300, 150, 50), GREEN, "source column 200");
    assert_eq!(at(&pixels, 300, 299, 149), GREEN);
    assert_eq!(at(&pixels, 300, 0, 49), BLACK);
    assert_eq!(at(&pixels, 300, 0, 150), BLACK);
}

#[test]
fn a_still_put_on_the_canvas_reads_back_and_is_cleared_after() {
    let _lock = super::super::HARDWARE_TEST_LOCK.lock();
    let Some(device) = device() else {
        eprintln!("skip: no D3D11 device");
        return;
    };
    let mut canvas = Canvas::new(handle(&device), 64, 32).unwrap();
    assert!(canvas.read_last().unwrap().is_none(), "nothing placed yet");
    let placed = canvas.place_pixels(&solid(8, 8, RED), 8, 8).unwrap().placed;
    let mut still = canvas.read_last().unwrap().unwrap();
    for px in still.as_chunks_mut::<4>().0 {
        px.copy_from_slice(&GREEN);
    }
    let put = canvas.put_canvas(&still, placed).unwrap();
    assert_eq!(put.placed, placed);
    assert_eq!(canvas.read_last().unwrap().unwrap(), still);
    // The still's slot comes round again for a small picture: cleared first.
    for _ in 0..3 {
        canvas.place_pixels(&solid(8, 8, RED), 8, 8).unwrap();
    }
    canvas.place_pixels(&solid(8, 8, RED), 8, 8).unwrap();
    let pixels = canvas.read_last().unwrap().unwrap();
    assert_eq!(at(&pixels, 64, 0, 0), BLACK);
    assert_eq!(at(&pixels, 64, 28, 12), RED);
}

#[test]
fn a_zero_size_or_short_picture_is_refused() {
    let _lock = super::super::HARDWARE_TEST_LOCK.lock();
    let Some(device) = device() else {
        eprintln!("skip: no D3D11 device");
        return;
    };
    assert!(Canvas::new(handle(&device), 0, 10).is_err());
    let mut canvas = Canvas::new(handle(&device), 16, 16).unwrap();
    assert!(canvas.place_pixels(&[0; 12], 2, 2).is_err());
    assert!(canvas.place_pixels(&[], 0, 2).is_err());
    assert!(
        canvas
            .put_canvas(&[0; 16], CanvasPlacement::default())
            .is_err()
    );
}
