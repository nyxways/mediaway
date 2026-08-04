//! `SimpleBlock`/`Block` lacing: splits one block's payload into 1+ sub-frame
//! byte ranges. See crate-local `adr/0002-full-matroska-profile.md`.

#![forbid(unsafe_code)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "crate-private helper used by demux.rs; module itself is private"
)]

use crate::vint;
use smallvec::SmallVec;

/// Lacing mode from a block's flags byte (bits 1-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lacing {
    /// No lacing — the whole remaining body is one frame.
    None,
    /// Xiph lacing: each size (except the last) is a run of `0xFF` bytes
    /// (each contributing 255) terminated by a byte `< 255`.
    Xiph,
    /// Fixed-size lacing: all frames are the same size (`remaining / count`).
    FixedSize,
    /// EBML lacing: the first size is an unsigned VINT; subsequent sizes are
    /// signed VINT-encoded deltas from the previous size.
    Ebml,
}

impl Lacing {
    pub(crate) const fn from_flags(flags: u8) -> Self {
        match (flags >> 1) & 0x03 {
            0b01 => Self::Xiph,
            0b10 => Self::FixedSize,
            0b11 => Self::Ebml,
            _ => Self::None,
        }
    }
}

/// Split `body[lace_start..]` into sub-frame `(start, end)` byte ranges
/// (absolute offsets into `body`). Returns `None` on any malformed encoding —
/// callers must drop the block cleanly rather than guess.
pub(crate) fn split(
    body: &[u8],
    lace_start: usize,
    lacing: Lacing,
) -> Option<SmallVec<[(usize, usize); 8]>> {
    if matches!(lacing, Lacing::None) {
        if lace_start > body.len() {
            return None;
        }
        let mut single = SmallVec::new();
        single.push((lace_start, body.len()));
        return Some(single);
    }

    let frame_count = usize::from(*body.get(lace_start)?) + 1;
    let mut pos = lace_start + 1;
    if frame_count == 1 {
        // Degenerate but legal: no size fields at all, one frame takes the rest.
        let mut single = SmallVec::new();
        single.push((pos, body.len()));
        return Some(single);
    }
    let explicit_count = frame_count - 1;

    let mut sizes: SmallVec<[usize; 8]> = SmallVec::new();
    match lacing {
        Lacing::Xiph => {
            for _ in 0..explicit_count {
                let mut size = 0usize;
                loop {
                    let b = *body.get(pos)?;
                    pos += 1;
                    size = size.checked_add(usize::from(b))?;
                    if b != 255 {
                        break;
                    }
                }
                sizes.push(size);
            }
        }
        Lacing::FixedSize => {
            let total = body.len().checked_sub(pos)?;
            if total % frame_count != 0 {
                return None;
            }
            let each = total / frame_count;
            sizes.extend(std::iter::repeat_n(each, explicit_count));
        }
        Lacing::Ebml => {
            let (first, len0) = vint::decode_size(&body[pos..]).ok()?;
            pos += len0;
            let mut prev = i64::try_from(first.value).ok()?;
            sizes.push(usize::try_from(prev).ok()?);
            for _ in 1..explicit_count {
                let (v, len) = vint::decode_size(&body[pos..]).ok()?;
                pos += len;
                let bias = (1i64 << (7 * len as i64 - 1)) - 1;
                let delta = i64::try_from(v.value).ok()? - bias;
                prev = prev.checked_add(delta)?;
                sizes.push(usize::try_from(prev).ok()?);
            }
        }
        Lacing::None => unreachable!("handled above"),
    }

    let mut ranges = SmallVec::new();
    let mut off = pos;
    for &size in &sizes {
        let end = off.checked_add(size)?;
        if end > body.len() {
            return None;
        }
        ranges.push((off, end));
        off = end;
    }
    if off > body.len() {
        return None;
    }
    ranges.push((off, body.len())); // last frame takes whatever remains
    Some(ranges)
}

/// Encode `sizes` — every sub-frame's size **except the last** (the last
/// frame always takes whatever remains, matching [`split`]'s decode
/// convention) — as EBML lacing: the first size as a plain unsigned VINT,
/// each following size as a signed delta from the previous one. Exact
/// inverse of `split`'s `Lacing::Ebml` branch. `sizes` empty (a single-frame
/// "lace") writes nothing — the caller has no size fields to emit either.
pub(crate) fn encode_ebml_lace_sizes(sizes: &[usize], out: &mut Vec<u8>) {
    let Some((&first, rest)) = sizes.split_first() else {
        return;
    };
    vint::encode_size(first as u64, out);
    let mut prev = first as i64;
    for &size in rest {
        let size = size as i64;
        vint::encode_signed_delta(size - prev, out);
        prev = size;
    }
}

#[cfg(test)]
#[path = "lacing_tests.rs"]
mod tests;
