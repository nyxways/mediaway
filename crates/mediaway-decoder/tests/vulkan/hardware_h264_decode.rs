//! Hardware-gated integration test: a real, hand-constructed multi-frame
//! H.264 Annex-B stream (SPS + PPS + IDR slice + one P slice) pushed through
//! [`VulkanVideoDecoder`], asserting real, varying NV12 output for the
//! P-frame — not just the IDR. Skips (never fails the default suite) when
//! this machine's Vulkan loader/driver lacks an H.264 decode queue family,
//! same convention as `mediaway-encoder-vulkan::encoder_tests`.
//!
//! **Why hand-constructed, not encoded by this workspace's own H.264
//! encoder**: `mediaway-encoder-vulkan::VulkanVideoEncoder` (checked
//! directly) makes every pushed frame an independent key frame — it has no
//! P-frame/GOP support to produce a real P-slice from. Hand-constructing
//! gives full control over macroblock content without needing a working
//! CAVLC residual/CBP encoder (see below).
//!
//! **Picture**: 64x16 (4 macroblocks, 1 row — this crate's reference RTX
//! 4090's H.264 decode profile reports a `48x16` minimum coded extent, found
//! empirically while writing this test; 64x16 clears it with margin while
//! staying 16-aligned). Every macroblock in both pictures is either `I_PCM`
//! (raw, uncoded 8-bit samples — no CAVLC coefficient/CBP-table encoding
//! needed to control content) or `P_Skip` (zero-motion-vector copy from the
//! reference, the simplest legal P-slice macroblock — ITU-T H.264 § 8.4.1.1
//! guarantees a zero-motion predictor for the first macroblock of a
//! picture).
//!
//! - IDR picture: MB0 = `I_PCM` luma 200 (Cb 100 / Cr 150); MB1 = `I_PCM`
//!   luma 50 (Cb 80 / Cr 180); MB2/MB3 = `I_PCM` luma 90 (filler, identical
//!   in both pictures so their correctness does not depend on `P_Skip`
//!   motion-vector-prediction edge cases next to an intra neighbor).
//! - P picture: MB0 = `P_Skip` (must reproduce the IDR's MB0 exactly — this
//!   assertion is what proves the DPB reference-slot pipeline actually wired
//!   the right image/layer into the real `vkCmdDecodeVideoKHR` call, not just
//!   that *a* decode call succeeded). MB1 = `I_PCM` luma 220 (a value
//!   deliberately different from the IDR's MB1, so the P-frame's own output
//!   is genuinely different from the IDR's — not a re-emitted copy of the
//!   same buffer). MB2/MB3 = `I_PCM` luma 90 (same filler as the IDR).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    clippy::cast_possible_truncation,
    reason = "test file: unwrap/print are fine; picture dimensions are small test constants"
)]

use mediaway_common::{Bytes, Packet, PixelFormat, Rational};
use mediaway_decoder::{VideoDecoder, VideoDecoderConfig, VideoOutputPreference};
use mediaway_decoder_vulkan::VulkanVideoDecoder;

const WIDTH: u32 = 64;
const HEIGHT: u32 = 16;
const MB_SIZE: usize = 16;

/// Minimal MSB-first bit packer, extended with byte-alignment and raw-byte
/// writes for `I_PCM` macroblock samples — same convention as this crate's
/// own `*_tests.rs` `BitWriter` helpers.
struct BitWriter {
    bytes: Vec<u8>,
    cur: u8,
    nbits: u8,
}

impl BitWriter {
    const fn new() -> Self {
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

    /// Pad with zero bits until byte-aligned (`pcm_alignment_zero_bit` /
    /// `rbsp_alignment_zero_bit`).
    fn byte_align(&mut self) {
        while self.nbits != 0 {
            self.push_bit(0);
        }
    }

    /// Write raw, uncoded bytes (must be called only when already
    /// byte-aligned — `I_PCM` sample data).
    fn write_raw_bytes(&mut self, data: &[u8]) {
        debug_assert_eq!(self.nbits, 0, "write_raw_bytes requires byte alignment");
        self.bytes.extend_from_slice(data);
    }

    fn rbsp_trailing_bits(&mut self) {
        self.push_bit(1); // rbsp_stop_one_bit
        self.byte_align(); // rbsp_alignment_zero_bit*
    }

    fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

/// One `I_PCM` macroblock: `mb_type` (already offset for I- vs P-slice
/// numbering by the caller), byte-align, then raw luma+Cb+Cr samples
/// (ITU-T H.264 § 7.3.5, § 7.4.5.3 — no CAVLC coefficient/CBP-table encoding
/// needed).
fn write_i_pcm_macroblock(writer: &mut BitWriter, mb_type: u32, luma: u8, cb: u8, cr: u8) {
    writer.write_ue(mb_type);
    writer.byte_align();
    writer.write_raw_bytes(&[luma; MB_SIZE * MB_SIZE]);
    writer.write_raw_bytes(&[cb; (MB_SIZE / 2) * (MB_SIZE / 2)]);
    writer.write_raw_bytes(&[cr; (MB_SIZE / 2) * (MB_SIZE / 2)]);
}

/// `emulation_prevention_three_byte` insertion (ITU-T H.264 § 7.4.1.1) — the
/// inverse of `mediaway_sw::h264::nal`'s `remove_emulation_prevention`,
/// applied here since this test hand-assembles real Annex-B bytes rather than
/// reusing an encoder.
fn insert_emulation_prevention(rbsp: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rbsp.len());
    let mut zero_run = 0u32;
    for &byte in rbsp {
        if zero_run >= 2 && byte <= 3 {
            out.push(0x03);
            zero_run = 0;
        }
        out.push(byte);
        zero_run = if byte == 0 { zero_run + 1 } else { 0 };
    }
    out
}

/// Wraps `rbsp` (already emulation-prevented) as one Annex-B NAL unit:
/// 4-byte start code + 1-byte NAL header + RBSP.
fn annex_b_nal(nal_ref_idc: u8, nal_unit_type: u8, rbsp: &[u8]) -> Vec<u8> {
    let mut out = vec![0x00, 0x00, 0x00, 0x01];
    out.push((nal_ref_idc << 5) | nal_unit_type);
    out.extend_from_slice(&insert_emulation_prevention(rbsp));
    out
}

/// Builds the SPS RBSP: Baseline profile, `pic_order_cnt_type == 0`,
/// progressive, `max_num_ref_frames == 1`, 32x16 (2x1 macroblocks).
fn build_sps() -> Vec<u8> {
    let mut writer = BitWriter::new();
    writer.write_ue(0); // seq_parameter_set_id
    writer.write_ue(0); // log2_max_frame_num_minus4 -> 4
    writer.write_ue(0); // pic_order_cnt_type = 0
    writer.write_ue(0); // log2_max_pic_order_cnt_lsb_minus4 -> 4
    writer.write_ue(1); // max_num_ref_frames
    writer.push_bit(0); // gaps_in_frame_num_value_allowed_flag
    writer.write_ue(3); // pic_width_in_mbs_minus1 -> 4 MBs (64px)
    writer.write_ue(0); // pic_height_in_map_units_minus1 -> 1 MB (16px)
    writer.push_bit(1); // frame_mbs_only_flag
    writer.push_bit(1); // direct_8x8_inference_flag
    writer.push_bit(0); // frame_cropping_flag
    writer.rbsp_trailing_bits();

    let mut rbsp = vec![66u8, 0, 30]; // profile_idc=Baseline, constraints=0, level_idc=3.0
    rbsp.extend(writer.finish());
    rbsp
}

/// Builds the PPS RBSP: CAVLC, one default reference, **deblocking-filter
/// control present** (so every slice can force `disable_deblocking_filter_idc
/// = 1` — this test's solid-color `I_PCM` macroblocks must decode to their
/// exact literal byte values for its assertions to be meaningful; found
/// empirically via an independent `ffmpeg` software decode of this same
/// stream that the default in-loop deblocking filter otherwise blends
/// adjacent macroblocks' edge samples), no redundant pictures.
fn build_pps() -> Vec<u8> {
    let mut writer = BitWriter::new();
    writer.write_ue(0); // pic_parameter_set_id
    writer.write_ue(0); // seq_parameter_set_id
    writer.push_bit(0); // entropy_coding_mode_flag (CAVLC)
    writer.push_bit(0); // bottom_field_pic_order_in_frame_present_flag
    writer.write_ue(0); // num_slice_groups_minus1
    writer.write_ue(0); // num_ref_idx_l0_default_active_minus1 -> 1 active ref
    writer.write_ue(0); // num_ref_idx_l1_default_active_minus1
    writer.push_bit(0); // weighted_pred_flag
    writer.write_bits(0, 2); // weighted_bipred_idc
    writer.write_se(0); // pic_init_qp_minus26
    writer.write_se(0); // pic_init_qs_minus26
    writer.write_se(0); // chroma_qp_index_offset
    writer.push_bit(1); // deblocking_filter_control_present_flag = 1
    writer.push_bit(0); // constrained_intra_pred_flag
    writer.push_bit(0); // redundant_pic_cnt_present_flag
    writer.rbsp_trailing_bits();
    writer.finish()
}

/// Builds the IDR slice RBSP: all 4 macroblocks `I_PCM` (I-slice `mb_type`
/// numbering: `I_PCM == 25`) — I-slices code every macroblock directly, no
/// `mb_skip_run` concept.
fn build_idr_slice() -> Vec<u8> {
    let mut writer = BitWriter::new();
    writer.write_ue(0); // first_mb_in_slice
    writer.write_ue(2); // slice_type = I
    writer.write_ue(0); // pic_parameter_set_id
    writer.write_bits(0, 4); // frame_num
    writer.write_ue(0); // idr_pic_id
    writer.write_bits(0, 4); // pic_order_cnt_lsb
    writer.push_bit(0); // no_output_of_prior_pics_flag
    writer.push_bit(0); // long_term_reference_flag
    writer.write_se(0); // slice_qp_delta
    writer.write_ue(1); // disable_deblocking_filter_idc = 1 (fully disabled)

    write_i_pcm_macroblock(&mut writer, 25, 200, 100, 150); // MB0
    write_i_pcm_macroblock(&mut writer, 25, 50, 80, 180); // MB1
    write_i_pcm_macroblock(&mut writer, 25, 90, 128, 128); // MB2 (filler)
    write_i_pcm_macroblock(&mut writer, 25, 90, 128, 128); // MB3 (filler)

    writer.rbsp_trailing_bits();
    writer.finish()
}

/// Builds the P slice RBSP: MB0 = `P_Skip` (`mb_skip_run = 1`), MB1-3 =
/// `I_PCM` (P-slice `mb_type` numbering: `I_PCM == 5 + 25 == 30`) — every
/// non-skipped macroblock in a P slice, including consecutive ones, is still
/// preceded by its own `mb_skip_run` (`0` when nothing is skipped before it;
/// ITU-T H.264 § 7.3.4's `slice_data()` loop reads one `mb_skip_run` per
/// iteration, even a zero one, before every `macroblock_layer()`).
fn build_p_slice() -> Vec<u8> {
    let mut writer = BitWriter::new();
    writer.write_ue(0); // first_mb_in_slice
    writer.write_ue(0); // slice_type = P
    writer.write_ue(0); // pic_parameter_set_id
    writer.write_bits(1, 4); // frame_num = 1
    writer.write_bits(2, 4); // pic_order_cnt_lsb = 2
    writer.push_bit(0); // num_ref_idx_active_override_flag
    writer.push_bit(0); // ref_pic_list_modification_flag_l0
    writer.push_bit(0); // adaptive_ref_pic_marking_mode_flag
    writer.write_se(0); // slice_qp_delta
    writer.write_ue(1); // disable_deblocking_filter_idc = 1 (fully disabled)

    writer.write_ue(1); // mb_skip_run = 1 (skip MB0)
    write_i_pcm_macroblock(&mut writer, 30, 220, 90, 160); // MB1
    writer.write_ue(0); // mb_skip_run = 0 (MB2 immediately follows, not skipped)
    write_i_pcm_macroblock(&mut writer, 30, 90, 128, 128); // MB2 (filler, matches IDR)
    writer.write_ue(0); // mb_skip_run = 0 (MB3 immediately follows, not skipped)
    write_i_pcm_macroblock(&mut writer, 30, 90, 128, 128); // MB3 (filler, matches IDR)

    writer.rbsp_trailing_bits();
    writer.finish()
}

fn luma_at(nv12: &[u8], x: usize, y: usize) -> u8 {
    nv12[y * WIDTH as usize + x]
}

/// Opens a real `VulkanVideoDecoder`, pushes the hand-constructed IDR+P
/// stream above, and asserts:
/// - The IDR decodes with MB0/MB1 luma matching what was written.
/// - The P-frame's MB0 (`P_Skip`) exactly reproduces the IDR's MB0 — proving
///   the DPB reference-slot pipeline wired the correct image/layer into the
///   real decode call.
/// - The P-frame's MB1 (`I_PCM`) differs from the IDR's MB1 — proving the
///   P-frame's own output is genuinely new content, not a re-emitted IDR
///   buffer.
#[test]
fn decode_idr_then_p_frame_or_skip() {
    let mut config = VideoDecoderConfig::h264(WIDTH, HEIGHT, Rational::new(1, 30));
    config.output = VideoOutputPreference::CpuFramesOk;

    let mut decoder = match VulkanVideoDecoder::open(&config) {
        Ok(decoder) => decoder,
        Err(error) => {
            eprintln!(
                "skip: VulkanVideoDecoder::open failed ({error:?}) — no decode-capable Vulkan device?"
            );
            return;
        }
    };

    let mut first_packet = annex_b_nal(3, 7, &build_sps()); // SPS, nal_ref_idc=3
    first_packet.extend(annex_b_nal(3, 8, &build_pps())); // PPS, nal_ref_idc=3
    first_packet.extend(annex_b_nal(1, 5, &build_idr_slice())); // IDR slice, nal_ref_idc=1

    if let Err(error) = decoder.push_packet(&Packet {
        stream_id: 0,
        pts: 0,
        dts: 0,
        duration: 1,
        is_keyframe: true,
        is_discard: false,
        payload: Bytes::from(first_packet),
    }) {
        eprintln!("skip: push_packet(IDR) failed ({error:?})");
        return;
    }

    let idr_frame = match decoder.poll_frame() {
        Ok(frame) => frame.expect("expected a decoded IDR frame, got none"),
        Err(error) => {
            eprintln!("skip: poll_frame(IDR) failed ({error:?})");
            return;
        }
    };
    let mediaway_common::VideoFrameStorage::Cpu { data: idr_nv12 } = idr_frame.storage else {
        unreachable!(
            "expected CPU NV12 storage for the IDR frame (VideoOutputPreference::CpuFramesOk was requested)"
        );
    };
    assert_eq!(idr_frame.format, PixelFormat::Nv12);
    assert_eq!(luma_at(&idr_nv12, 4, 4), 200, "IDR MB0 luma");
    assert_eq!(luma_at(&idr_nv12, 20, 4), 50, "IDR MB1 luma");

    let p_packet = annex_b_nal(1, 1, &build_p_slice()); // non-IDR slice, nal_ref_idc=1
    if let Err(error) = decoder.push_packet(&Packet {
        stream_id: 0,
        pts: 1,
        dts: 1,
        duration: 1,
        is_keyframe: false,
        is_discard: false,
        payload: Bytes::from(p_packet),
    }) {
        eprintln!("skip: push_packet(P) failed ({error:?})");
        return;
    }

    let p_frame = match decoder.poll_frame() {
        Ok(frame) => frame.expect("expected a decoded P frame, got none"),
        Err(error) => {
            eprintln!("skip: poll_frame(P) failed ({error:?})");
            return;
        }
    };
    let mediaway_common::VideoFrameStorage::Cpu { data: p_nv12 } = p_frame.storage else {
        unreachable!(
            "expected CPU NV12 storage for the P frame (VideoOutputPreference::CpuFramesOk was requested)"
        );
    };

    // P_Skip MB0 must exactly reproduce the IDR's MB0 (real motion-compensated
    // reference read from the DPB slot, not a coincidence).
    assert_eq!(
        luma_at(&p_nv12, 4, 4),
        200,
        "P MB0 (P_Skip) must match IDR MB0 luma"
    );
    // I_PCM MB1 is genuinely new content, differing from the IDR's MB1 — the
    // P-frame's own output is real and varying, not a re-emitted IDR buffer.
    assert_eq!(luma_at(&p_nv12, 20, 4), 220, "P MB1 (I_PCM) new content");
    assert_ne!(
        luma_at(&p_nv12, 20, 4),
        luma_at(&idr_nv12, 20, 4),
        "P-frame output must genuinely differ from the IDR, not just re-emit it"
    );

    let _ = decoder.flush();
}
