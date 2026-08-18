//! Sans-IO MP4 demuxer — fMP4 + unfragmented `stbl`; optional `ClearKey` CENC.

#![forbid(unsafe_code)]

use crate::isobmff::{
    SencSample, StblSample, TrackEncryption, TrunSample, parse_header, parse_moof, parse_moov,
    parse_senc,
};
use crate::types::{Bytes, Sample, Track};
use crate::{INLINE_SAMPLES, INLINE_TRACKS};
use iso_cenc::{Pattern, decrypt_cenc, iv_from_8, iv_from_constant};
use smallvec::SmallVec;
use std::collections::VecDeque;

#[derive(Debug, Clone)]
struct PendingStbl {
    stream_id: u32,
    sample: StblSample,
}

#[derive(Debug, Clone)]
struct MdatPart {
    file_offset: u64,
    data: Bytes,
}

/// Sans-IO demuxer: `push_bytes` → `poll_packet`.
#[derive(Debug, Default)]
pub struct Demuxer {
    buffer: Vec<u8>,
    read_pos: usize,
    /// Absolute file offset of `buffer[0]`.
    file_base: u64,
    streams: SmallVec<[Track; INLINE_TRACKS]>,
    track_encryption: SmallVec<[Option<TrackEncryption>; INLINE_TRACKS]>,
    packets: VecDeque<Sample>,
    pending: SmallVec<[TrunSample; INLINE_SAMPLES]>,
    pending_senc: Vec<SencSample>,
    track_id: u32,
    base_dts: u64,
    stbl_pending: VecDeque<PendingStbl>,
    mdat_parts: Vec<MdatPart>,
    /// `ClearKey` content key (16 bytes) when set.
    decryption_key: Option<[u8; 16]>,
}

impl Demuxer {
    /// Empty demuxer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Supply a `ClearKey` AES-128 content key for CENC sample decrypt.
    pub const fn set_decryption_key(&mut self, key: [u8; 16]) {
        self.decryption_key = Some(key);
    }

    /// Clear any previously set decryption key.
    pub const fn clear_decryption_key(&mut self) {
        self.decryption_key = None;
    }

    /// Feed container bytes.
    pub fn push_bytes(&mut self, chunk: &[u8]) {
        self.buffer.extend_from_slice(chunk);
        self.pump();
        self.emit_stbl();
        self.compact();
    }

    /// Track list from `moov`.
    #[must_use]
    pub fn streams(&self) -> &[Track] {
        &self.streams
    }

    /// Next Sample.
    pub fn poll_packet(&mut self) -> Option<Sample> {
        self.packets.pop_front()
    }

    fn pump(&mut self) {
        while self.read_pos + 8 <= self.buffer.len() {
            let Some(hdr) = parse_header(&self.buffer[self.read_pos..]) else {
                break;
            };
            if self.read_pos + hdr.size > self.buffer.len() {
                break;
            }
            let payload = self.read_pos + hdr.header_len;
            let end = self.read_pos + hdr.size;
            match &hdr.typ.0 {
                b"moov" if self.streams.is_empty() => {
                    let tracks = parse_moov(&self.buffer[payload..end]);
                    self.streams.clear();
                    self.track_encryption.clear();
                    self.stbl_pending.clear();
                    for t in tracks {
                        let sid = t.info.id;
                        for s in t.samples {
                            self.stbl_pending.push_back(PendingStbl {
                                stream_id: sid,
                                sample: s,
                            });
                        }
                        self.track_encryption.push(t.encryption);
                        self.streams.push(t.info);
                    }
                }
                b"moof" => {
                    let info = parse_moof(&self.buffer[payload..end]);
                    self.track_id = info.track_id;
                    self.base_dts = info.base_dts;
                    self.pending = info.samples;
                    self.pending_senc.clear();
                    let iv_size = self
                        .track_encryption
                        .get(info.track_id as usize)
                        .and_then(|e| e.as_ref())
                        .map_or(0, |e| e.per_sample_iv_size);
                    // `senc` may sit under `traf` — re-scan moof for it.
                    self.pending_senc = find_senc_in_moof(&self.buffer[payload..end], iv_size);
                }
                b"mdat" => {
                    let file_off = self.file_base.saturating_add(payload as u64);
                    let retain = self.should_retain_mdat();
                    if !self.pending.is_empty() {
                        self.drain_mdat(payload, end);
                    }
                    if retain {
                        self.mdat_parts.push(MdatPart {
                            file_offset: file_off,
                            data: Bytes::copy_from_slice(&self.buffer[payload..end]),
                        });
                    }
                }
                _ => {}
            }
            self.read_pos = end;
        }
    }

    fn should_retain_mdat(&self) -> bool {
        self.streams.is_empty() || !self.stbl_pending.is_empty()
    }

    fn drain_mdat(&mut self, start: usize, end: usize) {
        let samples = std::mem::take(&mut self.pending);
        let senc = std::mem::take(&mut self.pending_senc);
        if samples.is_empty() {
            return;
        }
        let mut off = start;
        let mut dts = self.base_dts;
        let tid = self.track_id;
        let enc = self
            .track_encryption
            .get(tid as usize)
            .and_then(|e| e.as_ref())
            .cloned();
        for (i, s) in samples.into_iter().enumerate() {
            let e = off.saturating_add(s.size as usize);
            if e > end || e > self.buffer.len() {
                break;
            }
            let pts = pts_from_dts(dts as i64, s.cto);
            let mut payload = self.buffer[off..e].to_vec();
            let decrypt_ok = if let (Some(key), Some(tenc)) = (self.decryption_key, enc.as_ref())
                && tenc.is_protected
            {
                let senc_s = senc.get(i);
                decrypt_sample(&mut payload, key, tenc, senc_s).is_ok()
            } else {
                true
            };
            self.packets.push_back(Sample {
                stream_id: tid,
                pts,
                dts: dts as i64,
                duration: u64::from(s.duration),
                is_keyframe: s.key,
                is_discard: !decrypt_ok,
                payload: Bytes::from(payload),
            });
            off = e;
            dts = dts.saturating_add(u64::from(s.duration));
        }
    }

    fn emit_stbl(&mut self) {
        if self.stbl_pending.is_empty() {
            // Keep retained `mdat` parts until `moov`/`stbl` arrives (mdat-before-moov).
            return;
        }
        while let Some(front) = self.stbl_pending.front() {
            let need = front.sample.offset;
            let size = front.sample.size as usize;
            let Some(bytes) = self.read_file_range(need, size) else {
                break;
            };
            let Some(PendingStbl { stream_id, sample }) = self.stbl_pending.pop_front() else {
                break;
            };
            let pts = pts_from_dts(sample.dts, sample.cto);
            let mut payload = bytes.to_vec();
            let decrypt_ok = if let (Some(key), Some(Some(tenc))) = (
                self.decryption_key,
                self.track_encryption.get(stream_id as usize),
            ) && tenc.is_protected
            {
                decrypt_sample(&mut payload, key, tenc, None).is_ok()
            } else {
                true
            };
            self.packets.push_back(Sample {
                stream_id,
                pts,
                dts: sample.dts,
                duration: u64::from(sample.duration),
                is_keyframe: sample.key,
                is_discard: sample.discard || !decrypt_ok,
                payload: Bytes::from(payload),
            });
        }
        if self.stbl_pending.is_empty() {
            self.mdat_parts.clear();
        }
    }

    fn read_file_range(&self, file_offset: u64, size: usize) -> Option<Bytes> {
        for part in &self.mdat_parts {
            let part_end = part.file_offset.saturating_add(part.data.len() as u64);
            if file_offset >= part.file_offset && file_offset + size as u64 <= part_end {
                let local = (file_offset - part.file_offset) as usize;
                return Some(part.data.slice(local..local + size));
            }
        }
        // Also allow still-buffered bytes (not yet compacted).
        let buf_start = self.file_base;
        let buf_end = buf_start.saturating_add(self.buffer.len() as u64);
        if file_offset >= buf_start && file_offset + size as u64 <= buf_end {
            let local = (file_offset - buf_start) as usize;
            return Some(Bytes::copy_from_slice(&self.buffer[local..local + size]));
        }
        None
    }

    fn compact(&mut self) {
        if self.read_pos == 0 {
            return;
        }
        // Do not drop bytes still needed for pending stbl samples.
        if !self.stbl_pending.is_empty() {
            return;
        }
        if self.read_pos >= 64 * 1024 || self.read_pos * 2 >= self.buffer.len() {
            self.buffer.drain(..self.read_pos);
            self.file_base = self.file_base.saturating_add(self.read_pos as u64);
            self.read_pos = 0;
        }
    }
}

fn pts_from_dts(dts: i64, cto: i32) -> i64 {
    dts.saturating_add(i64::from(cto))
}

fn find_senc_in_moof(moof: &[u8], per_sample_iv_size: u8) -> Vec<SencSample> {
    let mut pos = 0;
    while pos + 8 <= moof.len() {
        let Some(hdr) = parse_header(&moof[pos..]) else {
            break;
        };
        if pos + hdr.size > moof.len() {
            break;
        }
        let body = &moof[pos + hdr.header_len..pos + hdr.size];
        if &hdr.typ.0 == b"traf" {
            let mut tpos = 0;
            while tpos + 8 <= body.len() {
                let Some(th) = parse_header(&body[tpos..]) else {
                    break;
                };
                if tpos + th.size > body.len() {
                    break;
                }
                if &th.typ.0 == b"senc" {
                    return parse_senc(
                        &body[tpos + th.header_len..tpos + th.size],
                        per_sample_iv_size,
                    );
                }
                tpos += th.size;
            }
        }
        pos += hdr.size;
    }
    Vec::new()
}

fn decrypt_sample(
    payload: &mut [u8],
    key: [u8; 16],
    tenc: &TrackEncryption,
    senc: Option<&SencSample>,
) -> Result<(), iso_cenc::Error> {
    let iv = if let Some(s) = senc {
        if s.iv.is_empty() {
            iv_from_constant(&tenc.constant_iv)?
        } else {
            match s.iv.len() {
                8 => {
                    let mut a = [0u8; 8];
                    a.copy_from_slice(&s.iv);
                    iv_from_8(&a)
                }
                16 => {
                    let mut a = [0u8; 16];
                    a.copy_from_slice(&s.iv);
                    a
                }
                _ => return Ok(()),
            }
        }
    } else if !tenc.constant_iv.is_empty() {
        iv_from_constant(&tenc.constant_iv)?
    } else {
        return Ok(());
    };
    let subs: &[iso_cenc::Subsample] = senc.map_or(&[], |s| s.subsamples.as_slice());
    decrypt_cenc(&key, &iv, Pattern::NONE, payload, subs)
}
