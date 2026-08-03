//! Minimal H.264 Annex-B SPS/PPS writer for the D3D12 native video-encode backend.
//!
//! D3D12 Video Encode writes only the **slice** NAL (Annex-B, driver-formatted) into the
//! compressed bitstream buffer starting at `FrameStartOffset`; the application is
//! responsible for its own SPS/PPS. This module hand-writes the minimal valid SPS/PPS for
//! the single supported configuration: H.264 Main profile, one all-intra IDR slice per
//! push, `pic_order_cnt_type == 2` (no POC LSB / delta tables needed), zero reference
//! frames. No CABAC, no VUI, no scaling lists.
//!
//! Field values mirror `FFmpeg`'s `d3d12va_encode_h264.c` sequence/picture parameter setup
//! (verified against the shipped `libavcodec/d3d12va_encode_h264.c`), simplified for the
//! all-intra/no-reference case this backend targets this stage.

#![forbid(unsafe_code)]

/// MSB-first bit writer producing raw RBSP bytes (before emulation prevention). Shared by
/// this file's H.264 SPS/PPS writer, [`super::bitstream_hevc`]'s VPS/SPS/PPS writer, and
/// [`super::bitstream_av1`]'s OBU writer — all three use the same MSB-first raw-bit
/// primitives (`f(n)`/`ue(v)`/`se(v)` for H.264/HEVC, `f(n)` only for this backend's AV1
/// scope); AV1 additionally needs [`Self::byte_align_zero`] for its zero-only alignment
/// padding (`ue(v)`/`se(v)` Exp-Golomb are H.264/HEVC-only, per Rec. ITU-T H.264 §9.1 /
/// Rec. ITU-T H.265 §9.2 — unused by the AV1 writer).
pub(super) struct RbspWriter {
    bytes: Vec<u8>,
    bit_buf: u8,
    bit_count: u8,
}

impl RbspWriter {
    pub(super) const fn new() -> Self {
        Self {
            bytes: Vec::new(),
            bit_buf: 0,
            bit_count: 0,
        }
    }

    pub(super) fn write_bit(&mut self, bit: u8) {
        self.bit_buf = (self.bit_buf << 1) | (bit & 1);
        self.bit_count += 1;
        if self.bit_count == 8 {
            self.bytes.push(self.bit_buf);
            self.bit_buf = 0;
            self.bit_count = 0;
        }
    }

    /// Write the low `n` bits of `value` MSB-first. `n` must be `<= 32`.
    pub(super) fn write_bits(&mut self, value: u32, n: u8) {
        for i in (0..n).rev() {
            self.write_bit(u8::from((value >> i) & 1 == 1));
        }
    }

    /// Write `n` zero bits. Unlike [`Self::write_bits`], `n` may exceed 32 — used for HEVC
    /// `profile_tier_level()`'s wide reserved-bits fields (e.g. 43 bits), which would
    /// overflow a `u32` shift.
    pub(super) fn write_zero_bits(&mut self, n: u32) {
        for _ in 0..n {
            self.write_bit(0);
        }
    }

    pub(super) fn write_u8(&mut self, value: u8) {
        self.write_bits(u32::from(value), 8);
    }

    /// `ue(v)` unsigned Exp-Golomb (Rec. ITU-T H.264 §9.1 / Rec. ITU-T H.265 §9.2).
    pub(super) fn write_ue(&mut self, value: u32) {
        let code = value + 1;
        let bits = 32 - code.leading_zeros();
        // `bits` is `1..=32` for any `u32 value` — always fits `u8`.
        self.write_bits(0, u8::try_from(bits - 1).unwrap_or(31));
        self.write_bits(code, u8::try_from(bits).unwrap_or(32));
    }

    /// `se(v)` signed Exp-Golomb (Rec. ITU-T H.264 §9.1.1 / Rec. ITU-T H.265 §9.2).
    pub(super) fn write_se(&mut self, value: i32) {
        let code = if value <= 0 {
            value.unsigned_abs() * 2
        } else {
            u32::try_from(value).unwrap_or(0) * 2 - 1
        };
        self.write_ue(code);
    }

    /// `rbsp_trailing_bits()`: stop bit + zero pad to a byte boundary.
    pub(super) fn rbsp_trailing_bits(&mut self) {
        self.write_bit(1);
        while self.bit_count != 0 {
            self.write_bit(0);
        }
    }

    /// AV1's `byte_alignment()` (AV1 Bitstream & Decoding Process Specification §5.3.5):
    /// zero-pad to a byte boundary with **no** leading stop bit — unlike
    /// [`Self::rbsp_trailing_bits`]'s H.264/HEVC "1 then zero" pattern. Used by
    /// [`super::bitstream_av1`].
    pub(super) fn byte_align_zero(&mut self) {
        while self.bit_count != 0 {
            self.write_bit(0);
        }
    }

    pub(super) fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

/// Apply emulation prevention (`00 00 0x` → `00 00 03 0x` for `x <= 3`) to `rbsp` and append
/// the result to `out`. Shared by this file's 1-byte-NAL-header Annex-B wrapper and
/// [`super::bitstream_hevc`]'s 2-byte-NAL-header one — the emulation-prevention rule itself
/// (Rec. ITU-T H.264 §7.4.1.1 / Rec. ITU-T H.265 §7.4.2) is identical across both codecs.
pub(super) fn push_rbsp_with_emulation_prevention(out: &mut Vec<u8>, rbsp: &[u8]) {
    let mut zero_run = 0u8;
    for &b in rbsp {
        if zero_run >= 2 && b <= 3 {
            out.push(0x03);
            zero_run = 0;
        }
        out.push(b);
        zero_run = if b == 0 { zero_run + 1 } else { 0 };
    }
}

/// H.264 Main profile `profile_idc`.
const PROFILE_IDC_MAIN: u8 = 77;

fn write_sps(
    w: &mut RbspWriter,
    width_mbs_minus1: u32,
    height_map_units_minus1: u32,
    level_idc: u8,
) {
    w.write_u8(PROFILE_IDC_MAIN);
    w.write_bits(0, 8); // constraint_set0..5_flag + reserved_zero_2bits
    w.write_u8(level_idc);
    w.write_ue(0); // seq_parameter_set_id
    // chroma_format_idc / bit_depth / scaling lists: omitted — implied for profile != {100,110,122,244}
    w.write_ue(0); // log2_max_frame_num_minus4
    w.write_ue(2); // pic_order_cnt_type == 2 (POC derived from frame_num; no extra fields)
    w.write_ue(0); // max_num_ref_frames — no reference frames used this stage
    w.write_bit(0); // gaps_in_frame_num_value_allowed_flag
    w.write_ue(width_mbs_minus1);
    w.write_ue(height_map_units_minus1);
    w.write_bit(1); // frame_mbs_only_flag
    w.write_bit(1); // direct_8x8_inference_flag
    w.write_bit(0); // frame_cropping_flag — caller guarantees MB-aligned width/height
    w.write_bit(0); // vui_parameters_present_flag
    w.rbsp_trailing_bits();
}

fn write_pps(w: &mut RbspWriter) {
    w.write_ue(0); // pic_parameter_set_id
    w.write_ue(0); // seq_parameter_set_id
    w.write_bit(0); // entropy_coding_mode_flag — CAVLC (matches D3D12_CODEC_CONFIG's no-CABAC flag)
    w.write_bit(0); // bottom_field_pic_order_in_frame_present_flag
    w.write_ue(0); // num_slice_groups_minus1
    w.write_ue(0); // num_ref_idx_l0_default_active_minus1
    w.write_ue(0); // num_ref_idx_l1_default_active_minus1
    w.write_bit(0); // weighted_pred_flag
    w.write_bits(0, 2); // weighted_bipred_idc
    w.write_se(0); // pic_init_qp_minus26 — actual QP comes from D3D12 CQP rate control
    w.write_se(0); // pic_init_qs_minus26
    w.write_se(0); // chroma_qp_index_offset
    w.write_bit(1); // deblocking_filter_control_present_flag
    w.write_bit(0); // constrained_intra_pred_flag
    w.write_bit(0); // redundant_pic_cnt_present_flag
    w.rbsp_trailing_bits();
}

/// Wrap RBSP bytes in a NAL header, apply emulation prevention (`00 00 0x` → `00 00 03 0x`
/// for `x <= 3`), and prepend an Annex-B 4-byte start code.
fn annex_b_nal(nal_ref_idc: u8, nal_unit_type: u8, rbsp: &[u8]) -> Vec<u8> {
    let header = (nal_ref_idc << 5) | nal_unit_type;
    let mut out = Vec::with_capacity(rbsp.len() + rbsp.len() / 2 + 5);
    out.extend_from_slice(&[0, 0, 0, 1, header]);
    push_rbsp_with_emulation_prevention(&mut out, rbsp);
    out
}

/// Build the Annex-B SPS + PPS byte sequence for one encode session.
///
/// `width_mbs_minus1` / `height_map_units_minus1` are `width / 16 - 1` and
/// `height / 16 - 1` (callers validate 16-pixel alignment before calling). `level_idc` is
/// the H.264 level (e.g. `51` for Level 5.1) matching the `D3D12_VIDEO_ENCODER_LEVELS_H264`
/// passed to `CreateVideoEncoderHeap`.
pub(super) fn build_h264_headers(
    width_mbs_minus1: u32,
    height_map_units_minus1: u32,
    level_idc: u8,
) -> Vec<u8> {
    let mut sps_w = RbspWriter::new();
    write_sps(
        &mut sps_w,
        width_mbs_minus1,
        height_map_units_minus1,
        level_idc,
    );
    let sps_rbsp = sps_w.finish();

    let mut pps_w = RbspWriter::new();
    write_pps(&mut pps_w);
    let pps_rbsp = pps_w.finish();

    let mut out = annex_b_nal(3, 7, &sps_rbsp); // nal_unit_type 7 = SPS
    out.extend(annex_b_nal(3, 8, &pps_rbsp)); // nal_unit_type 8 = PPS
    out
}
