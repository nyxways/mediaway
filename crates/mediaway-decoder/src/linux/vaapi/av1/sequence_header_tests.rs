#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test modules may unwrap"
)]

//! Round-trip fixtures for [`SequenceHeader::parse`].
//!
//! ADR-0003's own test plan names a round-trip against
//! `windows::d3d12_video_encode::bitstream_av1::write_sequence_header`'s *actual* output as
//! this crate's highest-value regression test — but that function is `pub(super)` inside a
//! Windows-only module of a different crate (`mediaway-encoder`), so it cannot be called
//! directly from this Linux-only test. [`Writer`]/[`build_sequence_header`] below are a local,
//! test-only re-implementation of the same field order (hand-derived from reading
//! `bitstream_av1.rs:132-192` this session, parameterized over the optional-tool flags this
//! parser must reject), preserving the cross-check's real value — a real, independently
//! reasoned bit layout, not just this parser's own assumptions echoed back at itself.

use super::*;

/// Minimal MSB-first bit writer, mirroring
/// `mediaway-encoder`'s `windows::d3d12_video_encode::bitstream::RbspWriter` shape (same
/// `write_bit`/`write_bits`/`rbsp_trailing_bits` semantics) — duplicated locally since that
/// type is private to a different, Windows-only crate module.
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

    fn trailing(&mut self) {
        self.bit(true);
        while self.n != 0 {
            self.bit(false);
        }
    }

    fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

/// Every optional coding-tool flag this parser rejects when set, plus `enable_order_hint`'s
/// own sub-flags — defaults to the fully-disabled, accepted configuration.
#[derive(Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "one bool per sequence-header optional-tool flag under test; a state machine \
              would obscure which flag each rejection test actually flips"
)]
struct SeqFlags {
    use_128x128_superblock: bool,
    enable_filter_intra: bool,
    enable_intra_edge_filter: bool,
    enable_interintra_compound: bool,
    enable_masked_compound: bool,
    enable_warped_motion: bool,
    enable_dual_filter: bool,
    enable_order_hint: bool,
    enable_jnt_comp: bool,
    enable_ref_frame_mvs: bool,
    enable_superres: bool,
    enable_cdef: bool,
    enable_restoration: bool,
    high_bitdepth: bool,
    mono_chrome: bool,
    film_grain_params_present: bool,
}

/// Build a `sequence_header_obu()` payload for `width`x`height`, single operating point,
/// `seq_level_idx == 0` (no tier bit), no timing/decoder-model/frame-id info, `color_config()`
/// always taking its non-BT.709/sRGB/IDENTITY branch (`color_description_present_flag == 0`).
fn build_sequence_header(width: u32, height: u32, flags: &SeqFlags) -> Vec<u8> {
    let mut w = Writer::new();
    w.bits(0, 3); // seq_profile
    w.bit(false); // still_picture
    w.bit(false); // reduced_still_picture_header
    w.bit(false); // timing_info_present_flag
    w.bit(false); // initial_display_delay_present_flag
    w.bits(0, 5); // operating_points_cnt_minus_1
    w.bits(0, 12); // operating_point_idc[0]
    w.bits(0, 5); // seq_level_idx[0] == 0 -> no seq_tier bit

    let width_bits = (u32::BITS - (width - 1).leading_zeros()).max(1);
    let height_bits = (u32::BITS - (height - 1).leading_zeros()).max(1);
    w.bits(width_bits - 1, 4);
    w.bits(height_bits - 1, 4);
    w.bits(width - 1, width_bits);
    w.bits(height - 1, height_bits);

    w.bit(false); // frame_id_numbers_present_flag
    w.bit(flags.use_128x128_superblock);
    w.bit(flags.enable_filter_intra);
    w.bit(flags.enable_intra_edge_filter);
    w.bit(flags.enable_interintra_compound);
    w.bit(flags.enable_masked_compound);
    w.bit(flags.enable_warped_motion);
    w.bit(flags.enable_dual_filter);
    w.bit(flags.enable_order_hint);
    if flags.enable_order_hint {
        w.bit(flags.enable_jnt_comp);
        w.bit(flags.enable_ref_frame_mvs);
    }
    w.bit(false); // seq_choose_screen_content_tools
    w.bit(false); // seq_force_screen_content_tools
    // seq_force_screen_content_tools == 0 -> seq_force_integer_mv branch not read.
    if flags.enable_order_hint {
        w.bits(0, 3); // order_hint_bits_minus_1 == 0 -> OrderHintBits == 1
    }
    w.bit(flags.enable_superres);
    w.bit(flags.enable_cdef);
    w.bit(flags.enable_restoration);

    w.bit(flags.high_bitdepth);
    w.bit(flags.mono_chrome);
    w.bit(false); // color_description_present_flag
    w.bit(false); // color_range
    w.bits(0, 2); // chroma_sample_position
    w.bit(false); // separate_uv_delta_q

    w.bit(flags.film_grain_params_present);
    w.trailing();
    w.finish()
}

#[test]
fn accepted_minimal_sequence_header_round_trips() {
    let bytes = build_sequence_header(64, 64, &SeqFlags::default());
    let seq = SequenceHeader::parse(&bytes).unwrap();
    assert_eq!(seq.seq_profile, 0);
    assert!(!seq.use_128x128_superblock);
    assert!(!seq.enable_order_hint);
    assert_eq!(seq.order_hint_bits, 0);
    assert_eq!(seq.frame_width_bits_minus_1, 5);
    assert_eq!(seq.frame_height_bits_minus_1, 5);
    assert_eq!(seq.max_frame_width_minus_1, 63);
    assert_eq!(seq.max_frame_height_minus_1, 63);
    assert_eq!(seq.width(), 64);
    assert_eq!(seq.height(), 64);
    assert_eq!(seq.seq_force_screen_content_tools, 0);
    assert_eq!(seq.seq_force_integer_mv, SELECT_VALUE);
    assert!(!seq.color_range);
    assert_eq!(seq.matrix_coefficients, u8::try_from(UNSPECIFIED).unwrap());
    assert_eq!(seq.chroma_sample_position, 0);
}

#[test]
fn accepted_with_order_hint_enabled() {
    let flags = SeqFlags {
        enable_order_hint: true,
        ..SeqFlags::default()
    };
    let bytes = build_sequence_header(1920, 1080, &flags);
    let seq = SequenceHeader::parse(&bytes).unwrap();
    assert!(seq.enable_order_hint);
    assert_eq!(seq.order_hint_bits, 1); // order_hint_bits_minus_1 == 0 -> OrderHintBits == 1
    assert_eq!(seq.width(), 1920);
    assert_eq!(seq.height(), 1080);
}

#[test]
fn rejects_non_main_profile() {
    // seq_profile = 1 (000 -> 001), rest irrelevant: rejected before any further read.
    assert_eq!(
        SequenceHeader::parse(&[0b0010_0000]),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn rejects_reduced_still_picture_header() {
    // seq_profile=0 (3 bits), still_picture=0 (1 bit), reduced_still_picture_header=1 (1 bit).
    assert_eq!(
        SequenceHeader::parse(&[0b0000_1000]),
        Err(DecodeError::Unsupported)
    );
}

#[test]
fn rejects_timing_info_present() {
    assert_eq!(
        SequenceHeader::parse(&[0b0000_0100]),
        Err(DecodeError::Unsupported)
    );
}

fn assert_flag_rejected(mutate: impl FnOnce(&mut SeqFlags)) {
    let mut flags = SeqFlags::default();
    mutate(&mut flags);
    let bytes = build_sequence_header(64, 64, &flags);
    assert_eq!(SequenceHeader::parse(&bytes), Err(DecodeError::Unsupported));
}

#[test]
fn rejects_every_disabled_optional_tool_when_signaled() {
    assert_flag_rejected(|f| f.enable_filter_intra = true);
    assert_flag_rejected(|f| f.enable_intra_edge_filter = true);
    assert_flag_rejected(|f| f.enable_interintra_compound = true);
    assert_flag_rejected(|f| f.enable_masked_compound = true);
    assert_flag_rejected(|f| f.enable_warped_motion = true);
    assert_flag_rejected(|f| f.enable_dual_filter = true);
    assert_flag_rejected(|f| f.enable_superres = true);
    assert_flag_rejected(|f| f.enable_cdef = true);
    assert_flag_rejected(|f| f.enable_restoration = true);
    assert_flag_rejected(|f| f.high_bitdepth = true);
    assert_flag_rejected(|f| f.mono_chrome = true);
    assert_flag_rejected(|f| f.film_grain_params_present = true);
    assert_flag_rejected(|f| {
        f.enable_order_hint = true;
        f.enable_jnt_comp = true;
    });
    assert_flag_rejected(|f| {
        f.enable_order_hint = true;
        f.enable_ref_frame_mvs = true;
    });
}

#[test]
fn truncated_input_is_invalid() {
    assert_eq!(SequenceHeader::parse(&[]), Err(DecodeError::InvalidInput));
}
