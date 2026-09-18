#![cfg(test)]
#![allow(clippy::expect_used, reason = "unit tests")]

use super::FrameDimensions;

#[test]
fn native_keeps_the_window_size_exactly() {
    assert_eq!(
        FrameDimensions::Native.pool_size(1137, 636),
        Some((1137, 636))
    );
}

#[test]
fn even_cropped_drops_only_the_odd_axis() {
    // The size that could not be encoded, measured on a live Unity editor window.
    assert_eq!(
        FrameDimensions::EvenCropped.pool_size(1137, 636),
        Some((1136, 636))
    );
    assert_eq!(
        FrameDimensions::EvenCropped.pool_size(1920, 989),
        Some((1920, 988))
    );
    assert_eq!(
        FrameDimensions::EvenCropped.pool_size(1921, 989),
        Some((1920, 988))
    );
}

#[test]
fn even_cropped_leaves_an_even_size_alone() {
    assert_eq!(
        FrameDimensions::EvenCropped.pool_size(2560, 1392),
        Some((2560, 1392))
    );
}

#[test]
fn a_size_that_crops_to_nothing_is_refused() {
    assert_eq!(FrameDimensions::EvenCropped.pool_size(1, 480), None);
    assert_eq!(FrameDimensions::Native.pool_size(0, 480), None);
}

#[test]
fn every_even_cropped_size_is_encodable() {
    for w in 2..64 {
        for h in 2..64 {
            let (pw, ph) = FrameDimensions::EvenCropped
                .pool_size(w, h)
                .expect("non-empty");
            assert!(pw % 2 == 0 && ph % 2 == 0, "{w}x{h} -> {pw}x{ph}");
            assert!(
                w - pw <= 1 && h - ph <= 1,
                "{w}x{h} lost more than one pixel"
            );
        }
    }
}
