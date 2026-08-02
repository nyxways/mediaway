//! Shared ISOBMFF primitives — paired write/parse per box family (no `Box`).

#![forbid(unsafe_code)]

pub mod buf;
pub mod cenc_box;
pub mod elst;
pub mod fourcc;
pub mod fragment;
pub mod ftyp;
pub mod header;
pub mod moov;
pub mod sample_entry;
pub mod stbl;

pub use buf::ByteSource;
pub use cenc_box::{SencSample, TrackEncryption, parse_senc, parse_tenc};
pub use elst::{EditListEntry, expand_samples_by_edit_list, parse_edts, parse_elst};
pub use fourcc::{FourCc, tag};
pub use fragment::{MoofInfo, TrunSample, parse_moof, write_fragment};
pub use ftyp::write_ftyp;
pub use header::{Header, parse_header, write_box};
pub use moov::{MoovTrack, parse_moov, write_moov};
pub use stbl::{StblSample, parse_stbl_samples};
