//! H.264 SPS picture geometry: the coded size and the `frame_cropping` rectangle, read and
//! rewritten without touching any other field.
//!
//! An SPS is a few dozen bytes of Exp-Golomb codes (ITU-T H.264 § 7.3.2.1.1). It is parsed up to
//! the crop fields, a new crop is written, and every bit after the old crop (the VUI) is copied as
//! it was, so the rewritten NAL may be a byte or two longer or shorter. The parse reads the same
//! fields `FFmpeg`'s `libavcodec/h264_ps.c` (`ff_h264_decode_seq_parameter_set`, LGPL; read for
//! reference only, no code taken) does, scaling lists and every `pic_order_cnt_type` included.
//!
//! Ported from gway's Recoway (`app/recoway/crates/recoway-core/src/avc.rs`,
//! <https://github.com/nyxways/gway>, MIT OR Apache-2.0, same authors), which rewrites the crop
//! of its own recordings.

/// The `frame_cropping` offsets of an SPS, in luma samples from each edge of the coded picture
/// (equations 7-19 to 7-22 already applied). All zero for an SPS without a crop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SpsCrop {
    /// Columns cut from the left.
    pub left: u32,
    /// Columns cut from the right.
    pub right: u32,
    /// Rows cut from the top.
    pub top: u32,
    /// Rows cut from the bottom.
    pub bottom: u32,
}

impl SpsCrop {
    /// Whether the shown picture does not start at the coded picture's top-left.
    #[must_use]
    pub const fn has_offset(&self) -> bool {
        self.left > 0 || self.top > 0
    }
}

/// What an SPS says about its pictures' size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpsGeometry {
    /// The coded width: whole macroblocks.
    pub coded_width: u32,
    /// The coded height: whole macroblocks (macroblock pairs for interlaced streams).
    pub coded_height: u32,
    /// The crop.
    pub crop: SpsCrop,
    /// The crop's step in luma samples, horizontal and vertical (2 x 2 for 4:2:0 progressive):
    /// every crop offset is a multiple of it.
    pub crop_unit: (u32, u32),
}

/// Profiles whose SPS carries `chroma_format_idc` and the fields after it (§ 7.3.2.1.1).
const HIGH_PROFILES: [u32; 13] = [100, 110, 122, 244, 44, 83, 86, 118, 128, 138, 139, 134, 135];

/// Where an SPS's crop is, and what it says.
struct Parsed {
    geometry: SpsGeometry,
    /// The RBSP (emulation prevention removed), without the NAL header byte.
    rbsp: Vec<u8>,
    /// Bit position of `frame_cropping_flag` in `rbsp`.
    crop_at: usize,
    /// Bit position just after the crop fields.
    after_crop: usize,
}

/// The geometry of the SPS NAL unit `nal` (header byte first, no start code or length prefix).
/// `None` when `nal` is not an SPS or is not one this can read.
#[must_use]
pub fn parse_sps_geometry(nal: &[u8]) -> Option<SpsGeometry> {
    parse(nal).map(|p| p.geometry)
}

/// The SPS NAL unit `nal` with its crop set to `crop`, every other field as it was.
///
/// `None` when `nal` is not an SPS this can read, when an offset is not a multiple of the crop's
/// step ([`SpsGeometry::crop_unit`]), or when the crop leaves nothing shown. An all-zero `crop`
/// clears `frame_cropping_flag`.
#[must_use]
pub fn sps_with_crop(nal: &[u8], crop: SpsCrop) -> Option<Vec<u8>> {
    let parsed = parse(nal)?;
    let geometry = parsed.geometry;
    let (ux, uy) = geometry.crop_unit;
    if [crop.left, crop.right].iter().any(|v| v % ux != 0)
        || [crop.top, crop.bottom].iter().any(|v| v % uy != 0)
        || crop.left.checked_add(crop.right)? >= geometry.coded_width
        || crop.top.checked_add(crop.bottom)? >= geometry.coded_height
    {
        return None;
    }
    let mut out = BitWriter::default();
    let mut bits = Bits::new(&parsed.rbsp);
    for _ in 0..parsed.crop_at {
        out.put(bits.bit()?);
    }
    if crop == SpsCrop::default() {
        out.put(0);
    } else {
        out.put(1);
        out.put_ue(crop.left / ux);
        out.put_ue(crop.right / ux);
        out.put_ue(crop.top / uy);
        out.put_ue(crop.bottom / uy);
    }
    // Everything after the crop up to the stop bit; the stop bit and the alignment are written
    // again, since the new crop moved them.
    let stop = stop_bit(&parsed.rbsp)?;
    let mut rest = Bits::new(&parsed.rbsp);
    rest.pos = parsed.after_crop;
    while rest.pos < stop {
        out.put(rest.bit()?);
    }
    out.put(1);
    let rbsp = out.finish();
    let mut nal_out = Vec::with_capacity(rbsp.len() + 4);
    nal_out.push(*nal.first()?);
    escape_into(&rbsp, &mut nal_out);
    Some(nal_out)
}

fn parse(nal: &[u8]) -> Option<Parsed> {
    if nal.first()? & 0x1f != 7 {
        return None;
    }
    let rbsp = unescape(nal.get(1..)?);
    let mut b = Bits::new(&rbsp);
    let profile = b.bits(8)?;
    b.bits(16)?; // constraint flags, level
    b.ue()?; // seq_parameter_set_id
    let mut chroma_format = 1;
    let mut separate_planes = false;
    if HIGH_PROFILES.contains(&profile) {
        chroma_format = b.ue()?;
        if chroma_format == 3 {
            separate_planes = b.bit()? == 1;
        }
        b.ue()?; // bit_depth_luma_minus8
        b.ue()?; // bit_depth_chroma_minus8
        b.bit()?; // qpprime_y_zero_transform_bypass_flag
        if b.bit()? == 1 {
            let lists = if chroma_format == 3 { 12 } else { 8 };
            for i in 0..lists {
                if b.bit()? == 1 {
                    skip_scaling_list(&mut b, if i < 6 { 16 } else { 64 })?;
                }
            }
        }
    }
    b.ue()?; // log2_max_frame_num_minus4
    match b.ue()? {
        0 => {
            b.ue()?; // log2_max_pic_order_cnt_lsb_minus4
        }
        1 => {
            b.bit()?; // delta_pic_order_always_zero_flag
            b.se()?; // offset_for_non_ref_pic
            b.se()?; // offset_for_top_to_bottom_field
            let cycle = b.ue()?;
            if cycle > 255 {
                return None;
            }
            for _ in 0..cycle {
                b.se()?;
            }
        }
        2 => {}
        _ => return None,
    }
    b.ue()?; // max_num_ref_frames
    b.bit()?; // gaps_in_frame_num_value_allowed_flag
    let width_mbs = b.ue()?.checked_add(1)?;
    let height_units = b.ue()?.checked_add(1)?;
    let frame_mbs_only = b.bit()?;
    if frame_mbs_only == 0 {
        b.bit()?; // mb_adaptive_frame_field_flag
    }
    b.bit()?; // direct_8x8_inference_flag
    let crop_at = b.pos;
    let field_factor = 2 - frame_mbs_only;
    let coded_width = width_mbs.checked_mul(16)?;
    let coded_height = height_units.checked_mul(16 * field_factor)?;
    // Equations 7-19 to 7-22: the crop is counted in chroma samples (in field pairs for
    // interlaced streams); ChromaArrayType 0 (monochrome or separate planes) counts luma.
    let (sub_w, sub_h) = match (separate_planes, chroma_format) {
        (true, _) | (false, 0 | 3) => (1, 1),
        (false, 1) => (2, 2),
        (false, 2) => (2, 1),
        _ => return None,
    };
    let crop_unit = (sub_w, sub_h * field_factor);
    let crop = if b.bit()? == 1 {
        let crop = SpsCrop {
            left: b.ue()?.checked_mul(crop_unit.0)?,
            right: b.ue()?.checked_mul(crop_unit.0)?,
            top: b.ue()?.checked_mul(crop_unit.1)?,
            bottom: b.ue()?.checked_mul(crop_unit.1)?,
        };
        if crop.left.checked_add(crop.right)? >= coded_width
            || crop.top.checked_add(crop.bottom)? >= coded_height
        {
            return None;
        }
        crop
    } else {
        SpsCrop::default()
    };
    let after_crop = b.pos;
    Some(Parsed {
        geometry: SpsGeometry {
            coded_width,
            coded_height,
            crop,
            crop_unit,
        },
        rbsp,
        crop_at,
        after_crop,
    })
}

/// Reads past one `scaling_list` (§ 7.3.2.1.1.1).
fn skip_scaling_list(b: &mut Bits<'_>, size: usize) -> Option<()> {
    let (mut last, mut next) = (8i32, 8i32);
    for _ in 0..size {
        if next != 0 {
            next = (last + b.se()? + 256).rem_euclid(256);
        }
        if next != 0 {
            last = next;
        }
    }
    Some(())
}

/// The bit position of the RBSP stop bit: the last 1 bit.
fn stop_bit(rbsp: &[u8]) -> Option<usize> {
    let (index, byte) = rbsp.iter().enumerate().rev().find(|(_, b)| **b != 0)?;
    Some(index * 8 + 7 - byte.trailing_zeros() as usize)
}

/// `data` with the emulation prevention bytes (`00 00 03`) taken out (§ 7.4.1).
fn unescape(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut zeros = 0;
    for &byte in data {
        if zeros >= 2 && byte == 3 {
            zeros = 0;
            continue;
        }
        out.push(byte);
        zeros = if byte == 0 { zeros + 1 } else { 0 };
    }
    out
}

/// `rbsp` with emulation prevention bytes put in, appended to `out`.
fn escape_into(rbsp: &[u8], out: &mut Vec<u8>) {
    let mut zeros = 0;
    for &byte in rbsp {
        if zeros >= 2 && byte <= 3 {
            out.push(3);
            zeros = 0;
        }
        out.push(byte);
        zeros = if byte == 0 { zeros + 1 } else { 0 };
    }
}

/// A reader of bits, most significant first.
struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Bits<'a> {
    const fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn bit(&mut self) -> Option<u32> {
        let byte = *self.data.get(self.pos / 8)?;
        let bit = (byte >> (7 - self.pos % 8)) & 1;
        self.pos += 1;
        Some(u32::from(bit))
    }

    fn bits(&mut self, n: u32) -> Option<u32> {
        let mut value = 0;
        for _ in 0..n {
            value = (value << 1) | self.bit()?;
        }
        Some(value)
    }

    /// An unsigned Exp-Golomb code (§ 9.1).
    fn ue(&mut self) -> Option<u32> {
        let mut zeros = 0;
        while self.bit()? == 0 {
            zeros += 1;
            if zeros > 31 {
                return None;
            }
        }
        let rest = u64::from(self.bits(zeros)?);
        u32::try_from((1u64 << zeros) - 1 + rest).ok()
    }

    /// A signed Exp-Golomb code (§ 9.1.1).
    fn se(&mut self) -> Option<i32> {
        let k = i64::from(self.ue()?);
        let value = if k % 2 == 1 { (k + 1) / 2 } else { -(k / 2) };
        i32::try_from(value).ok()
    }
}

/// A writer of bits, most significant first.
#[derive(Default)]
struct BitWriter {
    out: Vec<u8>,
    byte: u8,
    filled: u8,
}

impl BitWriter {
    fn put(&mut self, bit: u32) {
        self.byte = (self.byte << 1) | u8::from(bit & 1 == 1);
        self.filled += 1;
        if self.filled == 8 {
            self.out.push(self.byte);
            self.byte = 0;
            self.filled = 0;
        }
    }

    /// An unsigned Exp-Golomb code (§ 9.1).
    fn put_ue(&mut self, value: u32) {
        let coded = u64::from(value) + 1;
        let len = 64 - coded.leading_zeros();
        for _ in 1..len {
            self.put(0);
        }
        for i in (0..len).rev() {
            self.put(u32::from((coded >> i) & 1 == 1));
        }
    }

    /// The bytes, the last one padded with zeros.
    fn finish(mut self) -> Vec<u8> {
        if self.filled > 0 {
            self.byte <<= 8 - self.filled;
            self.out.push(self.byte);
        }
        self.out
    }
}

#[cfg(test)]
#[path = "sps_tests.rs"]
mod tests;
