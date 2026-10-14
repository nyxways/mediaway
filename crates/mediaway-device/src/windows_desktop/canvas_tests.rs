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

#[test]
fn a_part_keeps_its_offset_and_is_cut_where_the_picture_ends() {
    let whole = CanvasPlacement::part((160, 120), (400, 300), (100, 50));
    assert_eq!(
        whole,
        CanvasPlacement {
            x: 0,
            y: 0,
            width: 160,
            height: 120,
            source_x: 100,
            source_y: 50
        }
    );
    // The picture shrank to 200 x 100: 100 x 50 of the part is left, at the canvas's top-left.
    let cut = CanvasPlacement::part((160, 120), (200, 100), (100, 50));
    assert_eq!((cut.x, cut.y, cut.width, cut.height), (0, 0, 100, 50));
    assert_eq!((cut.source_x, cut.source_y), (100, 50));
    // Shrank past the part's top-left: nothing of it is left.
    assert!(CanvasPlacement::part((160, 120), (90, 300), (100, 50)).is_empty());
    assert!(CanvasPlacement::part((160, 120), (400, 50), (100, 50)).is_empty());
    assert!(!whole.is_empty());
}

#[test]
fn a_part_of_a_texture_lands_at_the_top_left_and_black_fills_what_the_picture_lost() {
    let _lock = super::super::HARDWARE_TEST_LOCK.lock();
    let Some(device) = device() else {
        eprintln!("skip: no D3D11 device");
        return;
    };
    // The source: a 400 x 300 picture, red left of column 150, green from it.
    let mut source = Canvas::new(handle(&device), 400, 300).unwrap();
    let mut picture = Vec::new();
    for _ in 0..300 {
        picture.extend(RED.repeat(150));
        picture.extend(GREEN.repeat(250));
    }
    let src = source.place_pixels(&picture, 400, 300).unwrap();
    let VideoFrameStorage::Gpu(GpuBufferHandle::DirectX11 { texture, .. }) = src.frame.storage
    else {
        panic!("a texture");
    };
    let mut canvas = Canvas::new(handle(&device), 160, 120).unwrap();
    let placed = canvas
        .place_texture_part(texture, (400, 300), (100, 50))
        .unwrap();
    assert_eq!((placed.frame.width, placed.frame.height), (160, 120));
    let pixels = canvas.read_last().unwrap().unwrap();
    assert_eq!(at(&pixels, 160, 0, 0), RED, "source (100, 50)");
    assert_eq!(at(&pixels, 160, 49, 119), RED, "source column 149");
    assert_eq!(at(&pixels, 160, 50, 0), GREEN, "source column 150");
    assert_eq!(at(&pixels, 160, 159, 119), GREEN);
    // The same texture said to hold only a 200 x 100 picture (the window shrank): the part keeps
    // its offset, and what the picture no longer reaches is black, in every slot it comes to.
    for _ in 0..4 {
        canvas
            .place_texture_part(texture, (200, 100), (100, 50))
            .unwrap();
    }
    let pixels = canvas.read_last().unwrap().unwrap();
    assert_eq!(at(&pixels, 160, 0, 0), RED);
    assert_eq!(at(&pixels, 160, 99, 49), GREEN);
    assert_eq!(at(&pixels, 160, 100, 0), BLACK, "right of the picture");
    assert_eq!(at(&pixels, 160, 0, 50), BLACK, "below it");
    // Past the part's top-left: all black, nothing copied.
    let empty = canvas
        .place_texture_part(texture, (80, 300), (100, 50))
        .unwrap();
    assert!(empty.placed.is_empty());
    let pixels = canvas.read_last().unwrap().unwrap();
    assert!(pixels.as_chunks::<4>().0.iter().all(|px| *px == BLACK));
}

#[test]
fn a_part_of_pixels_in_memory_is_placed_as_a_texture_part_is() {
    let _lock = super::super::HARDWARE_TEST_LOCK.lock();
    let Some(device) = device() else {
        eprintln!("skip: no D3D11 device");
        return;
    };
    let mut picture = Vec::new();
    for y in 0..100u32 {
        for _ in 0..100u32 {
            picture.extend(if y < 30 { RED } else { GREEN });
        }
    }
    let mut canvas = Canvas::new(handle(&device), 40, 40).unwrap();
    canvas
        .place_pixels_part(&picture, 100, 100, (10, 20))
        .unwrap();
    let pixels = canvas.read_last().unwrap().unwrap();
    assert_eq!(at(&pixels, 40, 0, 9), RED, "source row 29");
    assert_eq!(at(&pixels, 40, 39, 10), GREEN, "source row 30");
    // A narrower picture, 30 wide: 20 columns of the part are left.
    let narrow: Vec<u8> = picture
        .as_chunks::<400>()
        .0
        .iter()
        .flat_map(|row| row[..120].to_vec())
        .collect();
    let cut = canvas
        .place_pixels_part(&narrow, 30, 100, (10, 20))
        .unwrap();
    assert_eq!((cut.placed.width, cut.placed.height), (20, 40));
    let pixels = canvas.read_last().unwrap().unwrap();
    assert_eq!(at(&pixels, 40, 19, 39), GREEN);
    assert_eq!(at(&pixels, 40, 20, 0), BLACK);
}

#[test]
fn a_region_is_cut_to_the_picture_then_placed_centered() {
    // Inside the picture: placed like a picture of its own size.
    let inside = CanvasPlacement::region((400, 300), (1000, 800), (100, 50, 200, 100));
    assert_eq!(
        (inside.x, inside.y, inside.width, inside.height),
        (100, 100, 200, 100)
    );
    assert_eq!((inside.source_x, inside.source_y), (100, 50));
    // Past the picture's right edge: only what the picture holds (150 columns), centered by that
    // size: (400 - 150) / 2 = 125, rounded down to the even offset 124.
    let cut = CanvasPlacement::region((400, 300), (250, 800), (100, 50, 200, 100));
    assert_eq!((cut.x, cut.width, cut.source_x), (124, 150, 100));
    assert_eq!((cut.y, cut.height), (100, 100));
    // Larger than the canvas: cut around its center, the source offset moves with it.
    let large = CanvasPlacement::region((400, 300), (2000, 1000), (100, 50, 1000, 100));
    assert_eq!((large.x, large.width, large.source_x), (0, 400, 100 + 300));
    // Starting outside the picture: nothing.
    assert!(CanvasPlacement::region((400, 300), (90, 800), (100, 50, 200, 100)).is_empty());
    assert!(CanvasPlacement::region((400, 300), (1000, 40), (100, 50, 200, 100)).is_empty());
    // A region the size of the picture at the origin is the picture, as `centered` has it.
    assert_eq!(
        CanvasPlacement::region((400, 300), (160, 120), (0, 0, 160, 120)),
        CanvasPlacement::centered((400, 300), (160, 120))
    );
}

#[test]
fn a_region_that_changes_size_lands_centered_and_leaves_no_trace() {
    let _lock = super::super::HARDWARE_TEST_LOCK.lock();
    let Some(device) = device() else {
        eprintln!("skip: no D3D11 device");
        return;
    };
    // The source: a 400 x 300 picture, red left of column 150, green from it.
    let mut source = Canvas::new(handle(&device), 400, 300).unwrap();
    let mut picture = Vec::new();
    for _ in 0..300 {
        picture.extend(RED.repeat(150));
        picture.extend(GREEN.repeat(250));
    }
    let src = source.place_pixels(&picture, 400, 300).unwrap();
    let VideoFrameStorage::Gpu(GpuBufferHandle::DirectX11 { texture, .. }) = src.frame.storage
    else {
        panic!("a texture");
    };
    let mut canvas = Canvas::new(handle(&device), 200, 160).unwrap();
    // 100 x 60 from (100, 50): 50 red columns then 50 green, centered at (50, 50).
    let first = canvas
        .place_texture_region(texture, (400, 300), (100, 50, 100, 60))
        .unwrap();
    assert_eq!((first.placed.x, first.placed.y), (50, 50));
    let pixels = canvas.read_last().unwrap().unwrap();
    assert_eq!(at(&pixels, 200, 50, 50), RED);
    assert_eq!(at(&pixels, 200, 99, 109), RED);
    assert_eq!(at(&pixels, 200, 100, 50), GREEN);
    assert_eq!(at(&pixels, 200, 149, 109), GREEN);
    assert_eq!(at(&pixels, 200, 49, 50), BLACK, "left of the region");
    assert_eq!(at(&pixels, 200, 50, 110), BLACK, "below it");
    // The region becomes smaller: what the larger one held is black again, in every slot.
    for _ in 0..4 {
        canvas
            .place_texture_region(texture, (400, 300), (100, 50, 60, 40))
            .unwrap();
    }
    let pixels = canvas.read_last().unwrap().unwrap();
    assert_eq!(at(&pixels, 200, 70, 60), RED);
    assert_eq!(
        at(&pixels, 200, 50, 50),
        BLACK,
        "where the larger region began"
    );
    assert_eq!(
        at(&pixels, 200, 129, 99),
        GREEN,
        "the smaller region's last pixel"
    );
    assert_eq!(
        at(&pixels, 200, 130, 60),
        BLACK,
        "right of it, where the larger one reached"
    );
    assert_eq!(
        at(&pixels, 200, 70, 100),
        BLACK,
        "below it, where the larger one reached"
    );
    // Outside the picture: an all-black canvas.
    let empty = canvas
        .place_texture_region(texture, (80, 300), (100, 50, 60, 40))
        .unwrap();
    assert!(empty.placed.is_empty());
    let pixels = canvas.read_last().unwrap().unwrap();
    assert!(pixels.as_chunks::<4>().0.iter().all(|px| *px == BLACK));
}

#[test]
fn a_region_of_pixels_in_memory_is_placed_as_a_texture_region_is() {
    let _lock = super::super::HARDWARE_TEST_LOCK.lock();
    let Some(device) = device() else {
        eprintln!("skip: no D3D11 device");
        return;
    };
    let mut picture = Vec::new();
    for y in 0..100u32 {
        for _ in 0..100u32 {
            picture.extend(if y < 30 { RED } else { GREEN });
        }
    }
    let mut canvas = Canvas::new(handle(&device), 60, 60).unwrap();
    let placed = canvas
        .place_pixels_region(&picture, 100, 100, (10, 20, 40, 20))
        .unwrap();
    assert_eq!((placed.placed.x, placed.placed.y), (10, 20));
    let pixels = canvas.read_last().unwrap().unwrap();
    assert_eq!(at(&pixels, 60, 10, 20), RED, "source row 20");
    assert_eq!(at(&pixels, 60, 49, 29), RED, "source row 29");
    assert_eq!(at(&pixels, 60, 10, 30), GREEN, "source row 30");
    assert_eq!(at(&pixels, 60, 9, 20), BLACK);
}
