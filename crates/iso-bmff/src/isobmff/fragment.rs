//! Fragmented media (`moof`/`traf`/`trun`/`mdat`) — write + parse together.

#![forbid(unsafe_code)]

use super::{FourCc, parse_header};
use crate::INLINE_SAMPLES;
use smallvec::SmallVec;

/// One sample row decoded from `trun` (or queued for emit).
#[derive(Debug, Clone, Copy)]
pub struct TrunSample {
    /// Duration in media timescale.
    pub duration: u32,
    /// Size in bytes.
    pub size: u32,
    /// Sync sample when the key bit is set in flags.
    pub key: bool,
    /// Composition time offset (`PTS - DTS`).
    pub cto: i32,
}

/// Result of parsing one `moof` (track + base decode time + sample table).
#[derive(Debug, Clone, Default)]
pub struct MoofInfo {
    /// 0-based stream id (`tfhd.track_id - 1`).
    pub track_id: u32,
    /// Base decode time from `tfdt`.
    pub base_dts: u64,
    /// Samples in order (inline up to [`INLINE_SAMPLES`]).
    pub samples: SmallVec<[TrunSample; INLINE_SAMPLES]>,
}

/// Write one `moof` + `mdat` fragment.
#[allow(clippy::too_many_arguments)]
pub fn write_fragment(
    buf: &mut Vec<u8>,
    sequence: u32,
    track_id: u32,
    base_dts: u64,
    durations: &[u32],
    sizes: &[u32],
    flags: &[u32],
    ctos: &[i32],
    payload: &[u8],
) {
    let n = durations.len() as u32;
    let trun_size = 20 + n.saturating_mul(16);
    let traf_size = 8 + 16 + 20 + trun_size;
    let moof_size = 8 + 16 + traf_size;
    let mdat_size = 8 + payload.len() as u32;
    let data_offset = moof_size + 8;

    buf.reserve((moof_size + mdat_size) as usize);
    buf.extend_from_slice(&moof_size.to_be_bytes());
    buf.extend_from_slice(b"moof");
    buf.extend_from_slice(&16u32.to_be_bytes());
    buf.extend_from_slice(b"mfhd");
    buf.extend_from_slice(&0u32.to_be_bytes());
    buf.extend_from_slice(&sequence.to_be_bytes());
    buf.extend_from_slice(&traf_size.to_be_bytes());
    buf.extend_from_slice(b"traf");
    buf.extend_from_slice(&16u32.to_be_bytes());
    buf.extend_from_slice(b"tfhd");
    buf.extend_from_slice(&0x0002_0000u32.to_be_bytes());
    buf.extend_from_slice(&track_id.to_be_bytes());
    buf.extend_from_slice(&20u32.to_be_bytes());
    buf.extend_from_slice(b"tfdt");
    buf.extend_from_slice(&0x0100_0000u32.to_be_bytes());
    buf.extend_from_slice(&base_dts.to_be_bytes());
    buf.extend_from_slice(&trun_size.to_be_bytes());
    buf.extend_from_slice(b"trun");
    let tr_flags: u32 = 0x0000_0001 | 0x0000_0100 | 0x0000_0200 | 0x0000_0400 | 0x0000_0800;
    buf.extend_from_slice(&tr_flags.to_be_bytes());
    buf.extend_from_slice(&n.to_be_bytes());
    buf.extend_from_slice(&data_offset.to_be_bytes());
    for i in 0..durations.len() {
        buf.extend_from_slice(&durations[i].to_be_bytes());
        buf.extend_from_slice(&sizes[i].to_be_bytes());
        buf.extend_from_slice(&flags[i].to_be_bytes());
        buf.extend_from_slice(&ctos[i].to_be_bytes());
    }
    buf.extend_from_slice(&mdat_size.to_be_bytes());
    buf.extend_from_slice(b"mdat");
    buf.extend_from_slice(payload);
}

/// Parse a `moof` box payload (after `moof` header) into track/sample info.
#[must_use]
pub fn parse_moof(moof_payload: &[u8]) -> MoofInfo {
    let mut info = MoofInfo::default();
    let mut pos = 0;
    while pos + 8 <= moof_payload.len() {
        let Some(hdr) = parse_header(&moof_payload[pos..]) else {
            break;
        };
        if pos + hdr.size > moof_payload.len() {
            break;
        }
        let p = pos + hdr.header_len;
        let e = pos + hdr.size;
        if hdr.typ == FourCc(*b"traf") {
            parse_traf(&moof_payload[p..e], &mut info);
        }
        pos = e;
    }
    info
}

fn parse_traf(traf: &[u8], info: &mut MoofInfo) {
    let mut pos = 0;
    while pos + 8 <= traf.len() {
        let Some(hdr) = parse_header(&traf[pos..]) else {
            break;
        };
        if pos + hdr.size > traf.len() {
            break;
        }
        let p = pos + hdr.header_len;
        let box_end = pos + hdr.size;
        match &hdr.typ.0 {
            b"tfhd" => {
                let slice = &traf[p..box_end];
                if slice.len() >= 8 {
                    info.track_id = u32::from_be_bytes([slice[4], slice[5], slice[6], slice[7]])
                        .saturating_sub(1);
                }
            }
            b"tfdt" => {
                let slice = &traf[p..box_end];
                if slice.len() >= 12 {
                    info.base_dts = if slice[0] == 1 {
                        u64::from_be_bytes([
                            slice[4], slice[5], slice[6], slice[7], slice[8], slice[9], slice[10],
                            slice[11],
                        ])
                    } else {
                        u64::from(u32::from_be_bytes([slice[4], slice[5], slice[6], slice[7]]))
                    };
                }
            }
            b"trun" => {
                info.samples.extend(parse_trun(&traf[p..box_end]));
            }
            _ => {}
        }
        pos += hdr.size;
    }
}

/// Parse `trun` sample table (`FullBox` payload).
#[must_use]
pub(crate) fn parse_trun(slice: &[u8]) -> SmallVec<[TrunSample; INLINE_SAMPLES]> {
    let mut out = SmallVec::new();
    if slice.len() < 8 {
        return out;
    }
    let flags = u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]);
    let count = u32::from_be_bytes([slice[4], slice[5], slice[6], slice[7]]) as usize;
    let mut pos = 8;
    if flags & 0x0000_0001 != 0 {
        pos += 4;
    }
    if flags & 0x0000_0004 != 0 {
        pos += 4;
    }
    let has_d = flags & 0x0000_0100 != 0;
    let has_s = flags & 0x0000_0200 != 0;
    let has_f = flags & 0x0000_0400 != 0;
    let has_c = flags & 0x0000_0800 != 0;
    for _ in 0..count {
        let mut dur = 0u32;
        let mut size = 0u32;
        let mut fl = 0u32;
        let mut cto = 0i32;
        if has_d {
            if pos + 4 > slice.len() {
                break;
            }
            dur = u32::from_be_bytes([slice[pos], slice[pos + 1], slice[pos + 2], slice[pos + 3]]);
            pos += 4;
        }
        if has_s {
            if pos + 4 > slice.len() {
                break;
            }
            size = u32::from_be_bytes([slice[pos], slice[pos + 1], slice[pos + 2], slice[pos + 3]]);
            pos += 4;
        }
        if has_f {
            if pos + 4 > slice.len() {
                break;
            }
            fl = u32::from_be_bytes([slice[pos], slice[pos + 1], slice[pos + 2], slice[pos + 3]]);
            pos += 4;
        }
        if has_c {
            if pos + 4 > slice.len() {
                break;
            }
            cto = i32::from_be_bytes([slice[pos], slice[pos + 1], slice[pos + 2], slice[pos + 3]]);
            pos += 4;
        }
        out.push(TrunSample {
            duration: dur,
            size,
            key: fl & 0x0200_0000 != 0,
            cto,
        });
    }
    out
}
