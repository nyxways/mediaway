#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

//! Round-trip fixtures for [`FrameHeader::parse`], mirroring
//! [`super::sequence_header::tests`]'s approach: a local, test-only bit writer hand-derived
//! from reading `windows::d3d12_video_encode::bitstream_av1::write_frame_header`
//! (`bitstream_av1.rs:204-282`) this session (that function is private to a different,
//! Windows-only crate module, so it cannot be called directly from this Linux-only test).

use super::*;

struct Writer {
    bytes: Vec<u8>,
    buf: u8,
    n: u8,
}

impl Writer {
    const fn new() -> Self {
        Self {
            bytes: Vec::new(),
            buf: 0,
            n: 0,
        }
    }

    fn bit(&mut self, b: bool) {
        self.buf = (self.buf << 1) | u8::from(b);
        self.n += 1;
        if self.n == 8 {
            self.bytes.push(self.buf);
            self.buf = 0;
            self.n = 0;
        }
    }

    fn bits(&mut self, value: u32, n: u32) {
        for i in (0..n).rev() {
            self.bit((value >> i) & 1 == 1);
        }
    }

    fn finish(mut self) -> Vec<u8> {
        // Pad the final partial byte with zeros (this test builder never relies on trailing
        // padding bits being meaningful — the parser only ever consumes exactly the bits each
        // fixture defines).
        while self.n != 0 {
            self.bit(false);
        }
        self.bytes
    }
}

/// A minimal accepted [`SequenceHeader`] fixture: Main profile, 64x64 max, no order hint,
/// screen-content-tools forced off — matches
/// `sequence_header_tests::accepted_minimal_sequence_header_round_trips`'s parsed result.
fn base_seq() -> SequenceHeader {
    SequenceHeader {
        seq_profile: 0,
        use_128x128_superblock: false,
        enable_order_hint: false,
        order_hint_bits: 0,
        frame_width_bits_minus_1: 5,
        frame_height_bits_minus_1: 5,
        max_frame_width_minus_1: 63,
        max_frame_height_minus_1: 63,
        seq_force_screen_content_tools: 0,
        seq_force_integer_mv: 2,
        color_range: false,
        matrix_coefficients: 2,
        chroma_sample_position: 0,
        separate_uv_delta_q: false,
    }
}

/// Build an `uncompressed_header()` payload for a shown `KEY_FRAME` against [`base_seq`]'s
/// 64x64 sequence header (trivial single-superblock `tile_info()`, matching
/// `tile_info_tests::uniform_trivial_single_superblock_needs_only_the_flag_bit`).
fn build_frame_header(base_q_idx: u8, segmentation_enabled: bool) -> Vec<u8> {
    let mut w = Writer::new();
    w.bit(false); // show_existing_frame
    w.bits(0, 2); // frame_type == KEY_FRAME
    w.bit(true); // show_frame
    w.bit(true); // disable_cdf_update
    // seq_force_screen_content_tools == 0 -> allow_screen_content_tools not read (== 0).
    w.bit(false); // frame_size_override_flag
    // OrderHintBits == 0 -> order_hint reads nothing.
    w.bit(false); // render_and_frame_size_different
    // allow_screen_content_tools == 0 -> allow_intrabc not read.
    // disable_cdf_update == 1 -> disable_frame_end_update_cdf not read.

    w.bit(true); // tile_info(): uniform_tile_spacing_flag (trivial single superblock)

    // quantization_params()
    w.bits(u32::from(base_q_idx), 8);
    w.bit(false); // delta_q_y_dc: delta_coded
    w.bit(false); // delta_q_u_dc: delta_coded
    w.bit(false); // delta_q_u_ac: delta_coded
    w.bit(false); // using_qmatrix

    w.bit(segmentation_enabled); // segmentation_params()

    if !segmentation_enabled {
        if base_q_idx > 0 {
            w.bit(false); // delta_q_present
        }

        let coded_lossless = base_q_idx == 0;
        if !coded_lossless {
            w.bits(0, 6); // loop_filter_level[0]
            w.bits(0, 6); // loop_filter_level[1]
            w.bits(0, 3); // loop_filter_sharpness
            w.bit(false); // loop_filter_delta_enabled
            w.bit(false); // tx_mode_select
        }
        w.bit(false); // reduced_tx_set
    }

    w.finish()
}

#[test]
fn accepted_frame_header_not_lossless_round_trips() {
    let seq = base_seq();
    let bytes = build_frame_header(32, false);
    let fh = FrameHeader::parse(&bytes, &seq).unwrap();

    assert_eq!(fh.frame_width_minus1, 63);
    assert_eq!(fh.frame_height_minus1, 63);
    assert_eq!(fh.order_hint, 0);
    assert!(fh.disable_cdf_update);
    assert!(fh.disable_frame_end_update_cdf);
    assert!(!fh.allow_screen_content_tools);
    assert!(!fh.allow_intrabc);
    assert_eq!(fh.tile_info.sb_cols, 1);
    assert_eq!(fh.tile_info.sb_rows, 1);
    assert_eq!(fh.quantization.base_q_idx, 32);
    assert_eq!(fh.quantization.delta_q_y_dc, 0);
    assert!(!fh.quantization.using_qmatrix);
    assert!(!fh.delta_q_present);
    assert!(!fh.coded_lossless);
    assert_eq!(fh.loop_filter.level, [0, 0]);
    assert_eq!(fh.loop_filter.ref_deltas, DEFAULT_REF_DELTAS);
    assert_eq!(fh.tx_mode, 1); // TX_MODE_LARGEST
    assert!(!fh.reduced_tx_set);
}

#[test]
fn accepted_frame_header_lossless_skips_loop_filter_and_tx_mode_bits() {
    let seq = base_seq();
    let bytes = build_frame_header(0, false);
    let fh = FrameHeader::parse(&bytes, &seq).unwrap();

    assert!(fh.coded_lossless);
    assert!(!fh.delta_q_present); // base_q_idx == 0 -> not read, inferred false
    assert_eq!(fh.loop_filter.level, [0, 0]);
    assert_eq!(fh.loop_filter.ref_deltas, DEFAULT_REF_DELTAS);
    assert_eq!(fh.tx_mode, 0); // ONLY_4X4
}

#[test]
fn rejects_segmentation_enabled() {
    let seq = base_seq();
    let bytes = build_frame_header(32, true);
    assert_eq!(
        FrameHeader::parse(&bytes, &seq),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn rejects_show_existing_frame() {
    let seq = base_seq();
    assert_eq!(
        FrameHeader::parse(&[0b1000_0000], &seq),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn rejects_non_key_frame_type() {
    let seq = base_seq();
    // show_existing_frame = 0, frame_type = 01 (INTRA_ONLY_FRAME).
    assert_eq!(
        FrameHeader::parse(&[0b0010_0000], &seq),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn rejects_non_shown_frame() {
    let seq = base_seq();
    // show_existing_frame = 0, frame_type = 00 (KEY_FRAME), show_frame = 0.
    assert_eq!(
        FrameHeader::parse(&[0b0000_0000], &seq),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn truncated_input_is_invalid() {
    let seq = base_seq();
    assert_eq!(
        FrameHeader::parse(&[], &seq),
        Err(DecodeError::InvalidInput)
    );
}
