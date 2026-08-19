//! Hand-constructed bitstream fixtures exercising [`Header::parse`] against the real spec
//! syntax tables copied verbatim in this crate's own ADR Addendum — see that module's own doc
//! comment for the exact field order these fixtures follow.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap / expect"
)]

use super::*;

/// Pack a `Vec<u8>` of individual bit values (0/1, MSB-first) into bytes, zero-padding the last
/// byte if needed.
fn pack_bits(bits: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; bits.len().div_ceil(8)];
    for (i, &bit) in bits.iter().enumerate() {
        if bit != 0 {
            out[i / 8] |= 1 << (7 - (i % 8));
        }
    }
    out
}

fn push_bits(bits: &mut Vec<u8>, value: u32, width: u32) {
    for i in (0..width).rev() {
        bits.push(u8::from((value >> i) & 1 != 0));
    }
}

/// Builds a minimal, spec-valid `KEY_FRAME` `uncompressed_header()` bit sequence: 64x64,
/// `error_resilient_mode = 0`, `base_q_idx = 10` (non-lossless), single tile,
/// `header_size_in_bytes = 5`. Exactly 112 bits (14 bytes) — see the inline field-by-field
/// comments for the running total.
fn key_frame_bits() -> Vec<u8> {
    let mut bits = Vec::new();
    push_bits(&mut bits, 0b10, 2); // frame_marker
    push_bits(&mut bits, 0, 1); // profile_low_bit
    push_bits(&mut bits, 0, 1); // profile_high_bit -> Profile = 0
    push_bits(&mut bits, 0, 1); // show_existing_frame
    push_bits(&mut bits, 0, 1); // frame_type: KEY_FRAME
    push_bits(&mut bits, 1, 1); // show_frame
    push_bits(&mut bits, 0, 1); // error_resilient_mode
    push_bits(&mut bits, 0x49, 8); // frame_sync_code byte 0
    push_bits(&mut bits, 0x83, 8); // frame_sync_code byte 1
    push_bits(&mut bits, 0x42, 8); // frame_sync_code byte 2
    push_bits(&mut bits, 1, 3); // color_config: color_space = CS_BT_601 (not CS_RGB)
    push_bits(&mut bits, 0, 1); // color_config: color_range
    push_bits(&mut bits, 63, 16); // frame_size: width_minus_1 (width = 64)
    push_bits(&mut bits, 63, 16); // frame_size: height_minus_1 (height = 64)
    push_bits(&mut bits, 0, 1); // render_size: render_and_frame_size_different
    push_bits(&mut bits, 0, 1); // refresh_frame_context (error_resilient_mode == 0)
    push_bits(&mut bits, 1, 1); // frame_parallel_decoding_mode
    push_bits(&mut bits, 0, 2); // frame_context_idx
    push_bits(&mut bits, 0, 6); // loop_filter: level
    push_bits(&mut bits, 0, 3); // loop_filter: sharpness
    push_bits(&mut bits, 0, 1); // loop_filter: delta_enabled
    push_bits(&mut bits, 10, 8); // quantization: base_q_idx (nonzero -> not lossless)
    push_bits(&mut bits, 0, 1); // quantization: delta_q_y_dc delta_coded
    push_bits(&mut bits, 0, 1); // quantization: delta_q_uv_dc delta_coded
    push_bits(&mut bits, 0, 1); // quantization: delta_q_uv_ac delta_coded
    push_bits(&mut bits, 0, 1); // segmentation_enabled
    // tile_info: width=64 -> mi_cols=8 -> sb64_cols=1 -> min_log2_tile_cols == max_log2_tile_cols
    // == 0, so the tile_cols_log2 while loop reads no bits at all.
    push_bits(&mut bits, 0, 1); // tile_rows_log2
    push_bits(&mut bits, 5, 16); // header_size_in_bytes
    bits
}

#[test]
fn parse_accepts_minimal_key_frame_and_reads_every_field_correctly() {
    let data = pack_bits(&key_frame_bits());
    assert_eq!(data.len(), 14); // 112 bits, byte-aligned
    let ref_table = RefTable::new();
    let header = Header::parse(&data, &ref_table).unwrap();

    assert!(header.is_key);
    assert!(!header.error_resilient_mode);
    assert_eq!(header.refresh_frame_flags, 0xff);
    assert_eq!(header.ref_frame_idx, [0, 0, 0]);
    assert_eq!(header.width, 64);
    assert_eq!(header.height, 64);
    assert!(!header.refresh_frame_context);
    assert!(header.frame_parallel_decoding_mode);
    assert_eq!(header.frame_context_idx, 0);
    assert_eq!(header.reset_frame_context, 0);
    assert_eq!(header.loop_filter.level, 0);
    assert_eq!(header.loop_filter.sharpness, 0);
    assert_eq!(header.quantization.base_q_idx, 10);
    assert!(!header.quantization.lossless);
    assert_eq!(header.first_partition_size, 5);
    assert_eq!(header.frame_header_length_in_bytes, 14);
}

#[test]
fn parse_rejects_non_zero_profile() {
    let mut bits = Vec::new();
    push_bits(&mut bits, 0b10, 2); // frame_marker
    push_bits(&mut bits, 1, 1); // profile_low_bit
    push_bits(&mut bits, 0, 1); // profile_high_bit -> Profile = 1
    let data = pack_bits(&bits);
    let ref_table = RefTable::new();
    assert_eq!(
        Header::parse(&data, &ref_table),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn parse_rejects_show_existing_frame() {
    let mut bits = Vec::new();
    push_bits(&mut bits, 0b10, 2);
    push_bits(&mut bits, 0, 1);
    push_bits(&mut bits, 0, 1);
    push_bits(&mut bits, 1, 1); // show_existing_frame
    let data = pack_bits(&bits);
    let ref_table = RefTable::new();
    assert_eq!(
        Header::parse(&data, &ref_table),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn parse_rejects_hidden_frame_show_frame_zero() {
    let mut bits = Vec::new();
    push_bits(&mut bits, 0b10, 2);
    push_bits(&mut bits, 0, 1);
    push_bits(&mut bits, 0, 1);
    push_bits(&mut bits, 0, 1); // show_existing_frame
    push_bits(&mut bits, 0, 1); // frame_type: KEY_FRAME
    push_bits(&mut bits, 0, 1); // show_frame = 0
    let data = pack_bits(&bits);
    let ref_table = RefTable::new();
    assert_eq!(
        Header::parse(&data, &ref_table),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn parse_rejects_lossless_frame() {
    // Same shape as key_frame_bits() up through quantization_params(), but with base_q_idx = 0
    // and all deltas uncoded -> Lossless.
    let mut bits = Vec::new();
    push_bits(&mut bits, 0b10, 2);
    push_bits(&mut bits, 0, 1);
    push_bits(&mut bits, 0, 1);
    push_bits(&mut bits, 0, 1); // show_existing_frame
    push_bits(&mut bits, 0, 1); // frame_type: KEY_FRAME
    push_bits(&mut bits, 1, 1); // show_frame
    push_bits(&mut bits, 0, 1); // error_resilient_mode
    push_bits(&mut bits, 0x49, 8);
    push_bits(&mut bits, 0x83, 8);
    push_bits(&mut bits, 0x42, 8);
    push_bits(&mut bits, 1, 3); // color_space
    push_bits(&mut bits, 0, 1); // color_range
    push_bits(&mut bits, 63, 16);
    push_bits(&mut bits, 63, 16);
    push_bits(&mut bits, 0, 1); // render_size different
    push_bits(&mut bits, 0, 1); // refresh_frame_context
    push_bits(&mut bits, 1, 1); // frame_parallel_decoding_mode
    push_bits(&mut bits, 0, 2); // frame_context_idx
    push_bits(&mut bits, 0, 6); // loop_filter level
    push_bits(&mut bits, 0, 3); // loop_filter sharpness
    push_bits(&mut bits, 0, 1); // loop_filter delta_enabled
    push_bits(&mut bits, 0, 8); // base_q_idx = 0
    push_bits(&mut bits, 0, 1); // delta_q_y_dc coded = 0
    push_bits(&mut bits, 0, 1); // delta_q_uv_dc coded = 0
    push_bits(&mut bits, 0, 1); // delta_q_uv_ac coded = 0 -> Lossless == true
    let data = pack_bits(&bits);
    let ref_table = RefTable::new();
    assert_eq!(
        Header::parse(&data, &ref_table),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn parse_rejects_segmentation_enabled() {
    // Same shape as key_frame_bits() up through segmentation_params(), but with
    // segmentation_enabled = 1.
    let mut bits = Vec::new();
    push_bits(&mut bits, 0b10, 2);
    push_bits(&mut bits, 0, 1);
    push_bits(&mut bits, 0, 1);
    push_bits(&mut bits, 0, 1);
    push_bits(&mut bits, 0, 1);
    push_bits(&mut bits, 1, 1);
    push_bits(&mut bits, 0, 1);
    push_bits(&mut bits, 0x49, 8);
    push_bits(&mut bits, 0x83, 8);
    push_bits(&mut bits, 0x42, 8);
    push_bits(&mut bits, 1, 3);
    push_bits(&mut bits, 0, 1);
    push_bits(&mut bits, 63, 16);
    push_bits(&mut bits, 63, 16);
    push_bits(&mut bits, 0, 1);
    push_bits(&mut bits, 0, 1);
    push_bits(&mut bits, 1, 1);
    push_bits(&mut bits, 0, 2);
    push_bits(&mut bits, 0, 6);
    push_bits(&mut bits, 0, 3);
    push_bits(&mut bits, 0, 1);
    push_bits(&mut bits, 10, 8);
    push_bits(&mut bits, 0, 1);
    push_bits(&mut bits, 0, 1);
    push_bits(&mut bits, 0, 1);
    push_bits(&mut bits, 1, 1); // segmentation_enabled = 1
    let data = pack_bits(&bits);
    let ref_table = RefTable::new();
    assert_eq!(
        Header::parse(&data, &ref_table),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn parse_inter_frame_uses_ref_table_for_frame_size_with_refs() {
    let mut ref_table = RefTable::new();
    ref_table.refresh(0b0000_0001, 0, 64, 64); // slot 0 -> pool 0, 64x64 (mirrors a prior KEY_FRAME)

    let mut bits = Vec::new();
    push_bits(&mut bits, 0b10, 2); // frame_marker
    push_bits(&mut bits, 0, 1); // profile_low_bit
    push_bits(&mut bits, 0, 1); // profile_high_bit
    push_bits(&mut bits, 0, 1); // show_existing_frame
    push_bits(&mut bits, 1, 1); // frame_type: non-key
    push_bits(&mut bits, 1, 1); // show_frame (forces intra_only inferred 0, unread)
    push_bits(&mut bits, 0, 1); // error_resilient_mode
    push_bits(&mut bits, 1, 2); // reset_frame_context (error_resilient_mode == 0)
    push_bits(&mut bits, 0b0000_0010, 8); // refresh_frame_flags: refresh logical slot 1 only
    push_bits(&mut bits, 0, 3); // ref_frame_idx[0] = 0 (LAST -> slot 0)
    push_bits(&mut bits, 0, 1); // ref_frame_sign_bias[0]
    push_bits(&mut bits, 0, 3); // ref_frame_idx[1]
    push_bits(&mut bits, 0, 1); // ref_frame_sign_bias[1]
    push_bits(&mut bits, 0, 3); // ref_frame_idx[2]
    push_bits(&mut bits, 0, 1); // ref_frame_sign_bias[2]
    push_bits(&mut bits, 1, 1); // frame_size_with_refs: found_ref[0] = 1 -> use ref slot 0's size
    push_bits(&mut bits, 0, 1); // render_size: different = 0
    push_bits(&mut bits, 0, 1); // allow_high_precision_mv
    push_bits(&mut bits, 1, 1); // read_interpolation_filter: is_filter_switchable = 1
    push_bits(&mut bits, 0, 1); // refresh_frame_context
    push_bits(&mut bits, 1, 1); // frame_parallel_decoding_mode
    push_bits(&mut bits, 0, 2); // frame_context_idx
    push_bits(&mut bits, 0, 6); // loop_filter level
    push_bits(&mut bits, 0, 3); // loop_filter sharpness
    push_bits(&mut bits, 0, 1); // loop_filter delta_enabled
    push_bits(&mut bits, 10, 8); // base_q_idx
    push_bits(&mut bits, 0, 1); // delta_q_y_dc coded
    push_bits(&mut bits, 0, 1); // delta_q_uv_dc coded
    push_bits(&mut bits, 0, 1); // delta_q_uv_ac coded
    push_bits(&mut bits, 0, 1); // segmentation_enabled
    push_bits(&mut bits, 0, 1); // tile_rows_log2 (width=64 -> single tile, no tile_cols bits)
    push_bits(&mut bits, 7, 16); // header_size_in_bytes

    let data = pack_bits(&bits);
    let header = Header::parse(&data, &ref_table).unwrap();

    assert!(!header.is_key);
    assert_eq!(header.refresh_frame_flags, 0b0000_0010);
    assert_eq!(header.ref_frame_idx, [0, 0, 0]);
    assert_eq!(header.width, 64);
    assert_eq!(header.height, 64);
    assert_eq!(header.reset_frame_context, 1);
    assert_eq!(header.interpolation_filter, INTERPOLATION_FILTER_SWITCHABLE);
    assert_eq!(header.first_partition_size, 7);
}

#[test]
fn parse_inter_frame_errors_when_referencing_an_empty_ref_slot() {
    let ref_table = RefTable::new(); // nothing populated — as if no KEY_FRAME was ever decoded

    let mut bits = Vec::new();
    push_bits(&mut bits, 0b10, 2);
    push_bits(&mut bits, 0, 1);
    push_bits(&mut bits, 0, 1);
    push_bits(&mut bits, 0, 1); // show_existing_frame
    push_bits(&mut bits, 1, 1); // frame_type: non-key
    push_bits(&mut bits, 1, 1); // show_frame
    push_bits(&mut bits, 0, 1); // error_resilient_mode
    push_bits(&mut bits, 0, 2); // reset_frame_context
    push_bits(&mut bits, 0, 8); // refresh_frame_flags
    push_bits(&mut bits, 0, 3); // ref_frame_idx[0]
    push_bits(&mut bits, 0, 1); // ref_frame_sign_bias[0]
    push_bits(&mut bits, 0, 3);
    push_bits(&mut bits, 0, 1);
    push_bits(&mut bits, 0, 3);
    push_bits(&mut bits, 0, 1);
    push_bits(&mut bits, 1, 1); // found_ref[0] = 1, but ref_table is empty at slot 0

    let data = pack_bits(&bits);
    assert_eq!(
        Header::parse(&data, &ref_table),
        Err(DecodeError::InvalidInput)
    );
}
