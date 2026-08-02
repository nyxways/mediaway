//! Track encryption boxes (`tenc` / `senc`) — parse only (ISO/IEC 23001-7).

#![forbid(unsafe_code)]

use iso_cenc::Subsample;
use smallvec::SmallVec;

/// Default encryption parameters from `tenc`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackEncryption {
    /// Non-zero when samples are protected by default.
    pub is_protected: bool,
    /// Per-sample IV size in bytes (0 ⇒ constant IV).
    pub per_sample_iv_size: u8,
    /// Key ID (16 bytes).
    pub kid: [u8; 16],
    /// Constant IV when `per_sample_iv_size == 0` (8 or 16 bytes).
    pub constant_iv: SmallVec<[u8; 16]>,
}

/// One sample's IV + optional subsample map from `senc`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SencSample {
    /// Per-sample IV bytes (`per_sample_iv_size` long), or empty if constant IV.
    pub iv: SmallVec<[u8; 16]>,
    /// Empty ⇒ whole sample protected.
    pub subsamples: SmallVec<[Subsample; 4]>,
}

/// Parse `tenc` `FullBox` payload.
#[must_use]
pub fn parse_tenc(body: &[u8]) -> Option<TrackEncryption> {
    if body.len() < 20 {
        return None;
    }
    let mut pos = 4;
    // `body[0]` is FullBox version; v1 crypt/skip nibbles are not parsed yet.
    pos += 1;
    if pos + 18 > body.len() {
        return None;
    }
    let is_protected = body[pos] != 0;
    let per_sample_iv_size = body[pos + 1];
    let mut kid = [0u8; 16];
    kid.copy_from_slice(&body[pos + 2..pos + 18]);
    pos += 18;
    let mut constant_iv = SmallVec::new();
    if is_protected && per_sample_iv_size == 0 {
        if pos >= body.len() {
            return None;
        }
        let iv_size = body[pos] as usize;
        pos += 1;
        if pos + iv_size > body.len() || (iv_size != 8 && iv_size != 16) {
            return None;
        }
        constant_iv.extend_from_slice(&body[pos..pos + iv_size]);
    }
    Some(TrackEncryption {
        is_protected,
        per_sample_iv_size,
        kid,
        constant_iv,
    })
}

/// Parse `senc` `FullBox` payload given the track's per-sample IV size.
#[must_use]
pub fn parse_senc(body: &[u8], per_sample_iv_size: u8) -> Vec<SencSample> {
    if body.len() < 8 {
        return Vec::new();
    }
    let flags = u32::from_be_bytes([body[0], body[1], body[2], body[3]]);
    let use_subsamples = flags & 0x0000_0002 != 0;
    let count = u32::from_be_bytes([body[4], body[5], body[6], body[7]]) as usize;
    let iv_size = usize::from(per_sample_iv_size);
    let mut out = Vec::with_capacity(count);
    let mut pos = 8;
    for _ in 0..count {
        let mut iv = SmallVec::new();
        if iv_size > 0 {
            if pos + iv_size > body.len() {
                break;
            }
            iv.extend_from_slice(&body[pos..pos + iv_size]);
            pos += iv_size;
        }
        let mut subsamples = SmallVec::new();
        if use_subsamples {
            if pos + 2 > body.len() {
                break;
            }
            let n = u16::from_be_bytes([body[pos], body[pos + 1]]) as usize;
            pos += 2;
            for _ in 0..n {
                if pos + 6 > body.len() {
                    return out;
                }
                let clear = u16::from_be_bytes([body[pos], body[pos + 1]]);
                let protected = u32::from_be_bytes([
                    body[pos + 2],
                    body[pos + 3],
                    body[pos + 4],
                    body[pos + 5],
                ]);
                subsamples.push(Subsample {
                    clear_bytes: clear,
                    protected_bytes: protected,
                });
                pos += 6;
            }
        }
        out.push(SencSample { iv, subsamples });
    }
    out
}
