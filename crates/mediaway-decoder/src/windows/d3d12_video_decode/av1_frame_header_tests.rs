//! Pure sans-io unit tests for [`super::parse_frame_header`] against a hand-built fixture
//! matching `mediaway-encoder-windows`'s `bitstream_av1.rs::write_frame_header`'s own
//! field shape (constructed independently here, not by calling the encoder crate).

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "unit tests")]

use super::parse_frame_header;
use crate::DecodeError;
use crate::windows::d3d12_video_decode::av1_sequence_header::SequenceHeader;

#[derive(Default)]
struct BitWriter {
    bytes: Vec<u8>,
    bit_pos: usize,
}

impl BitWriter {
    fn write_bit(&mut self, bit: u32) {
        let byte_index = self.bit_pos / 8;
        if byte_index >= self.bytes.len() {
            self.bytes.push(0);
        }
        if bit != 0 {
            let shift = 7 - (self.bit_pos % 8);
            self.bytes[byte_index] |= 1 << shift;
        }
        self.bit_pos += 1;
    }

    fn write_bits(&mut self, value: u32, count: u32) {
        for i in (0..count).rev() {
            self.write_bit((value >> i) & 1);
        }
    }

    fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

const WIDTH: u32 = 352;
const HEIGHT: u32 = 288;

fn test_seq() -> SequenceHeader {
    SequenceHeader {
        max_frame_width: WIDTH,
        max_frame_height: HEIGHT,
        frame_width_bits: 9,
        frame_height_bits: 9,
        use_128x128_superblock: false,
        enable_filter_intra: false,
        enable_intra_edge_filter: false,
        enable_interintra_compound: false,
        enable_masked_compound: false,
        enable_dual_filter: false,
        enable_order_hint: false,
        enable_jnt_comp: false,
        enable_ref_frame_mvs: false,
        order_hint_bits: 0,
        separate_uv_delta_q: false,
    }
}

/// A sequence header whose resolution forces `TileColsLog2 > 0` (`max_frame_width ==
/// 8192`) — real multi-tile territory, out of this module's single-tile scope.
fn test_seq_forcing_multi_tile() -> SequenceHeader {
    SequenceHeader {
        max_frame_width: 8192,
        frame_width_bits: 13,
        ..test_seq()
    }
}

struct Ov {
    show_existing_frame: u32,
    frame_type: u32,
    show_frame: u32,
    segmentation_enabled: u32,
    using_qmatrix: u32,
    uniform_tile_spacing_flag: u32,
    base_q_idx: u32,
}

impl Default for Ov {
    fn default() -> Self {
        Self {
            show_existing_frame: 0,
            frame_type: 0, // KEY_FRAME
            show_frame: 1,
            segmentation_enabled: 0,
            using_qmatrix: 0,
            uniform_tile_spacing_flag: 1,
            base_q_idx: 60, // nonzero -> CodedLossless == false, exercises the general read paths
        }
    }
}

/// `tile_info()` (AV1 spec §5.9.15) for `seq`'s `max_frame_width`/`max_frame_height` at
/// `use_128x128_superblock == false`: `uniform_tile_spacing_flag` + one "stop" bit per
/// direction (`min_log2_tile_{cols,rows} < max_log2_tile_{cols,rows}` holds for both
/// [`test_seq`] and [`test_seq_forcing_multi_tile`] — same shape
/// `bitstream_av1.rs::write_tile_info` already demonstrates for its own validated
/// resolution range).
fn write_tile_info_bits(w: &mut BitWriter, uniform: bool) {
    w.write_bit(u32::from(uniform));
    if uniform {
        w.write_bit(0); // increment_tile_cols_log2 = 0 -> stop at the minimum
        w.write_bit(0); // increment_tile_rows_log2 = 0 -> stop at the minimum
    }
}

fn build(seq: &SequenceHeader, ov: &Ov) -> Vec<u8> {
    let mut w = BitWriter::default();
    w.write_bit(ov.show_existing_frame);
    w.write_bits(ov.frame_type, 2);
    w.write_bit(ov.show_frame);
    w.write_bit(1); // disable_cdf_update -> DisableFrameEndUpdateCdf inferred, not read
    w.write_bit(0); // frame_size_override_flag
    if seq.order_hint_bits > 0 {
        w.write_bits(0, seq.order_hint_bits);
    }
    w.write_bit(0); // render_and_frame_size_different

    write_tile_info_bits(&mut w, ov.uniform_tile_spacing_flag != 0);

    // quantization_params()
    w.write_bits(ov.base_q_idx, 8);
    w.write_bit(0); // DeltaQYDc: delta_coded
    // NumPlanes > 1, separate_uv_delta_q == false (test_seq) -> diff_uv_delta not read.
    w.write_bit(0); // DeltaQUDc: delta_coded
    w.write_bit(0); // DeltaQUAc: delta_coded
    w.write_bit(ov.using_qmatrix);

    w.write_bit(ov.segmentation_enabled);

    // delta_q_params(): base_q_idx > 0 -> delta_q_present is read.
    if ov.base_q_idx > 0 {
        w.write_bit(0); // delta_q_present = false -> delta_lf_params() reads nothing
    }

    // loop_filter_params(): CodedLossless == (base_q_idx == 0 here) -> false for the
    // default base_q_idx == 60 fixture, so the real fields are read.
    let coded_lossless = ov.base_q_idx == 0;
    if !coded_lossless {
        w.write_bits(0, 6); // loop_filter_level[0]
        w.write_bits(0, 6); // loop_filter_level[1]
        // both == 0 -> loop_filter_level[2]/[3] not read.
        w.write_bits(0, 3); // loop_filter_sharpness
        w.write_bit(0); // loop_filter_delta_enabled
    }

    // cdef_params()/lr_params(): zero bits (enable_cdef/enable_restoration always false).

    if !coded_lossless {
        w.write_bit(0); // tx_mode_select -> TxMode == TX_MODE_LARGEST
    }

    w.write_bit(0); // reduced_tx_set

    w.finish()
}

#[test]
fn accepts_the_encoder_shaped_fixture() {
    let bytes = build(&test_seq(), &Ov::default());
    let (fh, bits) = parse_frame_header(&bytes, &test_seq()).unwrap();
    assert_eq!(fh.width, WIDTH);
    assert_eq!(fh.height, HEIGHT);
    assert_eq!(fh.quantization.base_q_idx, 60);
    assert_eq!(fh.tx_mode, 1); // TX_MODE_LARGEST
    assert!(bits > 0);
    assert!(bits <= bytes.len() * 8);
}

#[test]
fn rejects_show_existing_frame() {
    let bytes = build(
        &test_seq(),
        &Ov {
            show_existing_frame: 1,
            ..Ov::default()
        },
    );
    assert!(matches!(
        parse_frame_header(&bytes, &test_seq()),
        Err(DecodeError::Unsupported)
    ));
}

#[test]
fn rejects_non_key_frame_type() {
    let bytes = build(
        &test_seq(),
        &Ov {
            frame_type: 1, // INTER_FRAME
            ..Ov::default()
        },
    );
    assert!(matches!(
        parse_frame_header(&bytes, &test_seq()),
        Err(DecodeError::Unsupported)
    ));
}

#[test]
fn rejects_show_frame_zero() {
    let bytes = build(
        &test_seq(),
        &Ov {
            show_frame: 0,
            ..Ov::default()
        },
    );
    assert!(matches!(
        parse_frame_header(&bytes, &test_seq()),
        Err(DecodeError::Unsupported)
    ));
}

#[test]
fn rejects_segmentation_enabled() {
    let bytes = build(
        &test_seq(),
        &Ov {
            segmentation_enabled: 1,
            ..Ov::default()
        },
    );
    assert!(matches!(
        parse_frame_header(&bytes, &test_seq()),
        Err(DecodeError::Unsupported)
    ));
}

#[test]
fn rejects_using_qmatrix() {
    let bytes = build(
        &test_seq(),
        &Ov {
            using_qmatrix: 1,
            ..Ov::default()
        },
    );
    assert!(matches!(
        parse_frame_header(&bytes, &test_seq()),
        Err(DecodeError::Unsupported)
    ));
}

#[test]
fn rejects_non_uniform_tile_spacing() {
    let bytes = build(
        &test_seq(),
        &Ov {
            uniform_tile_spacing_flag: 0,
            ..Ov::default()
        },
    );
    assert!(matches!(
        parse_frame_header(&bytes, &test_seq()),
        Err(DecodeError::Unsupported)
    ));
}

#[test]
fn rejects_multi_tile_resolution() {
    let seq = test_seq_forcing_multi_tile();
    let bytes = build(&seq, &Ov::default());
    assert!(matches!(
        parse_frame_header(&bytes, &seq),
        Err(DecodeError::Unsupported)
    ));
}

#[test]
fn rejects_truncated_input() {
    // show_existing_frame=0, frame_type=00 (KEY_FRAME), show_frame=1,
    // disable_cdf_update=1 (skips the later disable_frame_end_update_cdf read),
    // frame_size_override_flag=0, render_and_frame_size_different=0,
    // uniform_tile_spacing_flag=1 — a legal prefix that runs out of bits exactly at
    // tile_info()'s first "stop increment_tile_cols_log2" read.
    let bytes = [0b0001_1001u8];
    assert!(matches!(
        parse_frame_header(&bytes, &test_seq()),
        Err(DecodeError::InvalidInput)
    ));
}
