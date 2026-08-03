//! Movie header `moov` — write + parse in one place.

#![forbid(unsafe_code)]

use super::cenc_box::TrackEncryption;
use super::elst::{expand_samples_by_edit_list, parse_edts};
use super::sample_entry::{parse_sample_entry, write_stsd};
use super::stbl::parse_stbl_samples;
use super::{FourCc, StblSample, parse_header, tag, write_box};
use crate::INLINE_TRACKS;
use crate::types::{Bytes, Codec, Rational, Track};
use smallvec::SmallVec;

/// One `trak` after `moov` parse: stream metadata + optional sample table / CENC.
#[derive(Debug, Clone)]
pub struct MoovTrack {
    /// Public stream info.
    pub info: Track,
    /// Unfragmented samples (empty for pure fMP4 init); edit-list expanded when present.
    pub samples: Vec<StblSample>,
    /// Default track encryption from `tenc`, if present.
    pub encryption: Option<TrackEncryption>,
}

/// Write `moov` (+ `mvex`/`trex`) for fragmented MP4.
pub fn write_moov(buf: &mut Vec<u8>, tracks: &[Track]) {
    write_box(buf, tag::MOOV, |b| {
        write_mvhd(b, tracks.len());
        for t in tracks {
            write_trak(b, t);
        }
        write_box(b, tag::MVEX, |bx| {
            for t in tracks {
                write_box(bx, tag::TREX, |tr| {
                    tr.extend_from_slice(&0u32.to_be_bytes());
                    tr.extend_from_slice(&(t.id.saturating_add(1)).to_be_bytes());
                    tr.extend_from_slice(&1u32.to_be_bytes());
                    tr.extend_from_slice(&0u32.to_be_bytes());
                    tr.extend_from_slice(&0u32.to_be_bytes());
                    tr.extend_from_slice(&0u32.to_be_bytes());
                });
            }
        });
    });
}

/// Parse `moov` payload into tracks (metadata + sample tables when present).
#[must_use]
pub fn parse_moov(moov_payload: &[u8]) -> SmallVec<[MoovTrack; INLINE_TRACKS]> {
    let mut out = SmallVec::new();
    let mut movie_timescale = 1000u32;
    let mut pos = 0;
    while pos + 8 <= moov_payload.len() {
        let Some(hdr) = parse_header(&moov_payload[pos..]) else {
            break;
        };
        if pos + hdr.size > moov_payload.len() {
            break;
        }
        let body = &moov_payload[pos + hdr.header_len..pos + hdr.size];
        match &hdr.typ.0 {
            b"mvhd" => {
                if let Some(ts) = parse_mvhd_timescale(body) {
                    movie_timescale = ts;
                }
            }
            b"trak" => {
                if let Some(t) = parse_trak(body, movie_timescale) {
                    out.push(t);
                }
            }
            _ => {}
        }
        pos += hdr.size;
    }
    out
}

fn parse_mvhd_timescale(body: &[u8]) -> Option<u32> {
    let version = *body.first()?;
    let off = if version == 1 { 20usize } else { 12 };
    if body.len() < off + 4 {
        return None;
    }
    Some(u32::from_be_bytes([
        body[off],
        body[off + 1],
        body[off + 2],
        body[off + 3],
    ]))
}

fn write_mvhd(buf: &mut Vec<u8>, n_tracks: usize) {
    write_box(buf, tag::MVHD, |b| {
        b.extend_from_slice(&0u32.to_be_bytes());
        b.extend_from_slice(&0u32.to_be_bytes());
        b.extend_from_slice(&0u32.to_be_bytes());
        b.extend_from_slice(&1000u32.to_be_bytes());
        b.extend_from_slice(&0u32.to_be_bytes());
        b.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        b.extend_from_slice(&0x0100u16.to_be_bytes());
        b.extend_from_slice(&[0u8; 10]);
        identity_matrix(b);
        b.extend_from_slice(&[0u8; 24]);
        b.extend_from_slice(&(n_tracks as u32 + 1).to_be_bytes());
    });
}

fn identity_matrix(b: &mut Vec<u8>) {
    for v in [0x0001_0000u32, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000] {
        b.extend_from_slice(&v.to_be_bytes());
    }
}

fn write_trak(buf: &mut Vec<u8>, track: &Track) {
    let audio = matches!(track.codec, Codec::Aac | Codec::Opus);
    write_box(buf, tag::TRAK, |b| {
        write_box(b, tag::TKHD, |tk| {
            tk.extend_from_slice(&7u32.to_be_bytes());
            tk.extend_from_slice(&0u32.to_be_bytes());
            tk.extend_from_slice(&0u32.to_be_bytes());
            tk.extend_from_slice(&(track.id.saturating_add(1)).to_be_bytes());
            tk.extend_from_slice(&0u32.to_be_bytes());
            tk.extend_from_slice(&0u32.to_be_bytes());
            tk.extend_from_slice(&[0u8; 8]);
            tk.extend_from_slice(&0u16.to_be_bytes());
            tk.extend_from_slice(&0u16.to_be_bytes());
            tk.extend_from_slice(&0x0100u16.to_be_bytes());
            tk.extend_from_slice(&0u16.to_be_bytes());
            identity_matrix(tk);
            tk.extend_from_slice(&(track.width << 16).to_be_bytes());
            tk.extend_from_slice(&(track.height << 16).to_be_bytes());
        });
        write_box(b, tag::MDIA, |md| {
            write_box(md, tag::MDHD, |h| {
                h.extend_from_slice(&0u32.to_be_bytes());
                h.extend_from_slice(&0u32.to_be_bytes());
                h.extend_from_slice(&0u32.to_be_bytes());
                h.extend_from_slice(&track.time_base.den.to_be_bytes());
                h.extend_from_slice(&0u32.to_be_bytes());
                h.extend_from_slice(&0x55C4u16.to_be_bytes());
                h.extend_from_slice(&0u16.to_be_bytes());
            });
            write_box(md, tag::HDLR, |h| {
                h.extend_from_slice(&0u32.to_be_bytes());
                h.extend_from_slice(&0u32.to_be_bytes());
                if audio {
                    h.extend_from_slice(b"soun");
                    h.extend_from_slice(&[0u8; 12]);
                    h.extend_from_slice(b"SoundHandler\0");
                } else {
                    h.extend_from_slice(b"vide");
                    h.extend_from_slice(&[0u8; 12]);
                    h.extend_from_slice(b"VideoHandler\0");
                }
            });
            write_box(md, tag::MINF, |mf| {
                if audio {
                    write_box(mf, tag::SMHD, |s| {
                        s.extend_from_slice(&0u32.to_be_bytes());
                        s.extend_from_slice(&0u16.to_be_bytes());
                        s.extend_from_slice(&0u16.to_be_bytes());
                    });
                } else {
                    write_box(mf, tag::VMHD, |v| {
                        v.extend_from_slice(&1u32.to_be_bytes());
                        v.extend_from_slice(&0u16.to_be_bytes());
                        v.extend_from_slice(&[0u8; 6]);
                    });
                }
                write_box(mf, tag::DINF, |d| {
                    write_box(d, tag::DREF, |r| {
                        r.extend_from_slice(&0u32.to_be_bytes());
                        r.extend_from_slice(&1u32.to_be_bytes());
                        write_box(r, tag::URL, |u| {
                            u.extend_from_slice(&1u32.to_be_bytes());
                        });
                    });
                });
                write_box(mf, tag::STBL, |st| {
                    write_stsd(st, track);
                    for empty in [b"stts", b"stsc", b"stco"] {
                        write_box(st, FourCc(*empty), |e| {
                            e.extend_from_slice(&0u32.to_be_bytes());
                            e.extend_from_slice(&0u32.to_be_bytes());
                        });
                    }
                    // stsz needs version/flags + sample_size + sample_count (12B
                    // payload) — writing only 8B makes readers overread into the
                    // next atom ("overread end of atom 'stsz'", broken header).
                    write_box(st, FourCc(*b"stsz"), |e| {
                        e.extend_from_slice(&0u32.to_be_bytes()); // version/flags
                        e.extend_from_slice(&0u32.to_be_bytes()); // sample_size = 0
                        e.extend_from_slice(&0u32.to_be_bytes()); // sample_count = 0
                    });
                });
            });
        });
    });
}

fn parse_trak(trak: &[u8], movie_timescale: u32) -> Option<MoovTrack> {
    let mut id = 0u32;
    let mut timescale = 1000u32;
    let mut handler = *b"vide";
    let mut width = 0u32;
    let mut height = 0u32;
    let mut codec = Codec::H264;
    let mut extra = Bytes::new();
    let mut samples = Vec::new();
    let mut encryption = None;
    let mut edits = Vec::new();

    let mut pos = 0;
    while pos + 8 <= trak.len() {
        let hdr = parse_header(&trak[pos..])?;
        if pos + hdr.size > trak.len() {
            break;
        }
        let body = &trak[pos + hdr.header_len..pos + hdr.size];
        match &hdr.typ.0 {
            b"tkhd" if body.len() >= 16 => {
                let off = if body[0] == 1 { 20 } else { 12 };
                if body.len() >= off + 4 {
                    id = u32::from_be_bytes([
                        body[off],
                        body[off + 1],
                        body[off + 2],
                        body[off + 3],
                    ])
                    .saturating_sub(1);
                }
            }
            b"edts" => {
                edits = parse_edts(body);
            }
            b"mdia" => parse_mdia(
                body,
                &mut timescale,
                &mut handler,
                &mut width,
                &mut height,
                &mut codec,
                &mut extra,
                &mut samples,
                &mut encryption,
            ),
            _ => {}
        }
        pos += hdr.size;
    }

    if &handler == b"soun" && !matches!(codec, Codec::Aac | Codec::Opus) {
        codec = Codec::Aac;
    }

    if !edits.is_empty() && !samples.is_empty() {
        samples = expand_samples_by_edit_list(&samples, &edits, timescale, movie_timescale);
    }

    Some(MoovTrack {
        info: Track {
            id,
            codec,
            time_base: Rational::new(1, timescale),
            width,
            height,
            extra_data: extra,
        },
        samples,
        encryption,
    })
}

#[allow(clippy::too_many_arguments)]
fn parse_mdia(
    mdia: &[u8],
    timescale: &mut u32,
    handler: &mut [u8; 4],
    width: &mut u32,
    height: &mut u32,
    codec: &mut Codec,
    extra: &mut Bytes,
    samples: &mut Vec<StblSample>,
    encryption: &mut Option<TrackEncryption>,
) {
    let mut pos = 0;
    while pos + 8 <= mdia.len() {
        let Some(hdr) = parse_header(&mdia[pos..]) else {
            break;
        };
        if pos + hdr.size > mdia.len() {
            break;
        }
        let body = &mdia[pos + hdr.header_len..pos + hdr.size];
        match &hdr.typ.0 {
            b"mdhd" => {
                let off = if body.first() == Some(&1) { 20 } else { 12 };
                if body.len() >= off + 4 {
                    *timescale = u32::from_be_bytes([
                        body[off],
                        body[off + 1],
                        body[off + 2],
                        body[off + 3],
                    ]);
                }
            }
            b"hdlr" if body.len() >= 12 => {
                *handler = [body[8], body[9], body[10], body[11]];
            }
            b"minf" => parse_minf(body, width, height, codec, extra, samples, encryption),
            _ => {}
        }
        pos += hdr.size;
    }
}

#[allow(clippy::too_many_arguments)]
fn parse_minf(
    minf: &[u8],
    width: &mut u32,
    height: &mut u32,
    codec: &mut Codec,
    extra: &mut Bytes,
    samples: &mut Vec<StblSample>,
    encryption: &mut Option<TrackEncryption>,
) {
    let mut pos = 0;
    while pos + 8 <= minf.len() {
        let Some(hdr) = parse_header(&minf[pos..]) else {
            break;
        };
        if pos + hdr.size > minf.len() {
            break;
        }
        if &hdr.typ.0 == b"stbl" {
            let stbl = &minf[pos + hdr.header_len..pos + hdr.size];
            parse_stbl(stbl, width, height, codec, extra, encryption);
            *samples = parse_stbl_samples(stbl);
        }
        pos += hdr.size;
    }
}

fn parse_stbl(
    stbl: &[u8],
    width: &mut u32,
    height: &mut u32,
    codec: &mut Codec,
    extra: &mut Bytes,
    encryption: &mut Option<TrackEncryption>,
) {
    let mut pos = 0;
    while pos + 8 <= stbl.len() {
        let Some(hdr) = parse_header(&stbl[pos..]) else {
            break;
        };
        if pos + hdr.size > stbl.len() {
            break;
        }
        if &hdr.typ.0 == b"stsd" {
            let body = &stbl[pos + hdr.header_len..pos + hdr.size];
            if body.len() > 8 {
                parse_sample_entry(&body[8..], width, height, codec, extra, encryption);
            }
        }
        pos += hdr.size;
    }
}
