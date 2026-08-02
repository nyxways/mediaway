#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

use super::*;

/// Minimal MSB-first bit packer used only to build test bitstreams; mirrors the bit
/// order [`BitReader`] expects so tests build inputs the same way a real encoder would.
struct BitWriter {
    bytes: Vec<u8>,
    cur: u8,
    nbits: u8,
}

impl BitWriter {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            cur: 0,
            nbits: 0,
        }
    }

    fn push_bit(&mut self, bit: u32) {
        let bit_u8 = u8::from(bit & 1 == 1);
        self.cur = (self.cur << 1) | bit_u8;
        self.nbits += 1;
        if self.nbits == 8 {
            self.bytes.push(self.cur);
            self.cur = 0;
            self.nbits = 0;
        }
    }

    fn write_bits(&mut self, value: u32, count: u32) {
        for i in (0..count).rev() {
            self.push_bit(value >> i);
        }
    }

    fn write_ue(&mut self, value: u32) {
        let code = value + 1;
        let len = u32::BITS - code.leading_zeros();
        for _ in 0..(len - 1) {
            self.push_bit(0);
        }
        self.write_bits(code, len);
    }

    fn write_se(&mut self, value: i32) {
        let magnitude = value.unsigned_abs();
        let code = if value <= 0 {
            magnitude * 2
        } else {
            magnitude * 2 - 1
        };
        self.write_ue(code);
    }

    fn finish(mut self) -> Vec<u8> {
        while self.nbits != 0 {
            self.push_bit(0);
        }
        self.bytes
    }
}

#[test]
fn read_bit_reads_msb_first() {
    let data = [0b1010_0000u8];
    let mut reader = BitReader::new(&data);
    assert_eq!(reader.read_bit().unwrap(), 1);
    assert_eq!(reader.read_bit().unwrap(), 0);
    assert_eq!(reader.read_bit().unwrap(), 1);
    assert_eq!(reader.read_bit().unwrap(), 0);
}

#[test]
fn read_bits_spans_byte_boundary() {
    let data = [0b0000_0001u8, 0b1000_0000u8];
    let mut reader = BitReader::new(&data);
    // Skip 7 zero bits, then read 3 bits spanning the boundary: "1" + "10" = 0b110 = 6.
    reader.read_bits(7).unwrap();
    assert_eq!(reader.read_bits(3).unwrap(), 0b110);
}

#[test]
fn read_bit_errors_past_end_of_buffer() {
    let data = [0u8];
    let mut reader = BitReader::new(&data);
    reader.read_bits(8).unwrap();
    assert_eq!(reader.read_bit(), Err(H264Error::UnexpectedEof));
}

#[test]
fn read_ue_decodes_zero_as_single_one_bit() {
    let mut writer = BitWriter::new();
    writer.write_ue(0);
    let bytes = writer.finish();
    let mut reader = BitReader::new(&bytes);
    assert_eq!(reader.read_ue().unwrap(), 0);
}

#[test]
fn read_ue_roundtrips_values_via_bitwriter() {
    for value in [0u32, 1, 2, 7, 19, 255, 1_000] {
        let mut writer = BitWriter::new();
        writer.write_ue(value);
        let bytes = writer.finish();
        let mut reader = BitReader::new(&bytes);
        assert_eq!(reader.read_ue().unwrap(), value, "value={value}");
    }
}

#[test]
fn read_se_roundtrips_signed_values_via_bitwriter() {
    for value in [0i32, 1, -1, 2, -2, 3, -3, 26, -26] {
        let mut writer = BitWriter::new();
        writer.write_se(value);
        let bytes = writer.finish();
        let mut reader = BitReader::new(&bytes);
        assert_eq!(reader.read_se().unwrap(), value, "value={value}");
    }
}

#[test]
fn read_ue_errors_on_truncated_prefix() {
    // A single zero bit followed by end-of-data: the leading-zero-bit scan for ue(v)
    // never finds a terminating "1" bit within the buffer.
    let data = [0b0000_0000u8];
    let mut reader = BitReader::new(&data);
    assert_eq!(reader.read_ue(), Err(H264Error::UnexpectedEof));
}

#[test]
fn bits_read_tracks_consumed_bit_count() {
    let data = [0xFFu8, 0xFF];
    let mut reader = BitReader::new(&data);
    assert_eq!(reader.bits_read(), 0);
    reader.read_bits(5).unwrap();
    assert_eq!(reader.bits_read(), 5);
}
