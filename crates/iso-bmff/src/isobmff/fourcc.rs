//! Four-character code.

#![forbid(unsafe_code)]

/// ISOBMFF box type / brand (`ftyp`, `moov`, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FourCc(pub [u8; 4]);

impl FourCc {
    /// Construct from raw bytes.
    #[must_use]
    pub const fn new(tag: [u8; 4]) -> Self {
        Self(tag)
    }

    /// Big-endian `u32` form.
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        u32::from_be_bytes(self.0)
    }
}

impl PartialEq<[u8; 4]> for FourCc {
    fn eq(&self, other: &[u8; 4]) -> bool {
        &self.0 == other
    }
}

/// Common tags as constants.
pub mod tag {
    use super::FourCc;

    /// `ftyp`
    pub const FTYP: FourCc = FourCc(*b"ftyp");
    /// `moov`
    pub const MOOV: FourCc = FourCc(*b"moov");
    /// `trak`
    pub const TRAK: FourCc = FourCc(*b"trak");
    /// `mdia`
    pub const MDIA: FourCc = FourCc(*b"mdia");
    /// `minf`
    pub const MINF: FourCc = FourCc(*b"minf");
    /// `stbl`
    pub const STBL: FourCc = FourCc(*b"stbl");
    /// `stsd`
    pub const STSD: FourCc = FourCc(*b"stsd");
    /// `moof`
    pub const MOOF: FourCc = FourCc(*b"moof");
    /// `traf`
    pub const TRAF: FourCc = FourCc(*b"traf");
    /// `mdat`
    pub const MDAT: FourCc = FourCc(*b"mdat");
    /// `mfhd`
    pub const MFHD: FourCc = FourCc(*b"mfhd");
    /// `tfhd`
    pub const TFHD: FourCc = FourCc(*b"tfhd");
    /// `tfdt`
    pub const TFDT: FourCc = FourCc(*b"tfdt");
    /// `trun`
    pub const TRUN: FourCc = FourCc(*b"trun");
    /// `mvhd`
    pub const MVHD: FourCc = FourCc(*b"mvhd");
    /// `tkhd`
    pub const TKHD: FourCc = FourCc(*b"tkhd");
    /// `mdhd`
    pub const MDHD: FourCc = FourCc(*b"mdhd");
    /// `hdlr`
    pub const HDLR: FourCc = FourCc(*b"hdlr");
    /// `vmhd`
    pub const VMHD: FourCc = FourCc(*b"vmhd");
    /// `smhd`
    pub const SMHD: FourCc = FourCc(*b"smhd");
    /// `dinf`
    pub const DINF: FourCc = FourCc(*b"dinf");
    /// `dref`
    pub const DREF: FourCc = FourCc(*b"dref");
    /// `url `
    pub const URL: FourCc = FourCc(*b"url ");
    /// `mvex`
    pub const MVEX: FourCc = FourCc(*b"mvex");
    /// `trex`
    pub const TREX: FourCc = FourCc(*b"trex");
    /// `edts`
    pub const EDTS: FourCc = FourCc(*b"edts");
    /// `elst`
    pub const ELST: FourCc = FourCc(*b"elst");
    /// `avc1`
    pub const AVC1: FourCc = FourCc(*b"avc1");
    /// `avcC`
    pub const AVCC: FourCc = FourCc(*b"avcC");
    /// `mp4a`
    pub const MP4A: FourCc = FourCc(*b"mp4a");
    /// `esds`
    pub const ESDS: FourCc = FourCc(*b"esds");
    /// `vp09`
    pub const VP09: FourCc = FourCc(*b"vp09");
    /// `vpcC`
    pub const VPCC: FourCc = FourCc(*b"vpcC");
    /// `hvc1` (HEVC, out-of-band parameter sets — the common muxing mode)
    pub const HVC1: FourCc = FourCc(*b"hvc1");
    /// `hvcC`
    pub const HVCC: FourCc = FourCc(*b"hvcC");
    /// `av01`
    pub const AV01: FourCc = FourCc(*b"av01");
    /// `av1C`
    pub const AV1C: FourCc = FourCc(*b"av1C");
}
