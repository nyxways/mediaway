#![cfg(test)]

use super::nv12_byte_size;

#[test]
fn nv12_byte_size_is_one_and_a_half_bytes_per_pixel() {
    assert_eq!(nv12_byte_size(16, 16), 16 * 16 + 16 * 16 / 2);
    assert_eq!(nv12_byte_size(1920, 1080), 1920 * 1080 + 1920 * 1080 / 2);
}
