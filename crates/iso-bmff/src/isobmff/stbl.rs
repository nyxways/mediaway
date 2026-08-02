//! Unfragmented sample table (`stbl`: `stts`/`ctts`/`stsc`/`stsz`/`stco`/`co64`/`stss`).

#![forbid(unsafe_code)]

use super::{FourCc, parse_header};

/// One sample located via the sample table (file-absolute offset).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StblSample {
    /// Byte offset from the start of the file to the sample payload.
    pub offset: u64,
    /// Sample size in bytes.
    pub size: u32,
    /// Decode timestamp (media timescale; may be negative after edit-list remap).
    pub dts: i64,
    /// Duration in media timescale.
    pub duration: u32,
    /// Composition time offset (`PTS - DTS`).
    pub cto: i32,
    /// Sync / random-access sample.
    pub key: bool,
    /// Outside the active edit list window (marked for presentation discard).
    pub discard: bool,
}

/// Expand `stbl` children into ordered samples. Empty if tables are absent/empty (fMP4).
#[must_use]
pub fn parse_stbl_samples(stbl_payload: &[u8]) -> Vec<StblSample> {
    let mut stts: Vec<(u32, u32)> = Vec::new();
    let mut ctts: Vec<(u32, i32)> = Vec::new();
    let mut stsc: Vec<(u32, u32)> = Vec::new();
    let mut sizes: Vec<u32> = Vec::new();
    let mut default_size = 0u32;
    let mut sample_count = 0u32;
    let mut chunk_offs: Vec<u64> = Vec::new();
    let mut sync: Vec<u32> = Vec::new();
    let mut has_stss = false;

    let mut pos = 0;
    while pos + 8 <= stbl_payload.len() {
        let Some(hdr) = parse_header(&stbl_payload[pos..]) else {
            break;
        };
        if pos + hdr.size > stbl_payload.len() {
            break;
        }
        let body = &stbl_payload[pos + hdr.header_len..pos + hdr.size];
        match &hdr.typ.0 {
            b"stts" => stts = parse_stts(body),
            b"ctts" => ctts = parse_ctts(body),
            b"stsc" => stsc = parse_stsc(body),
            b"stsz" => {
                if let Some((def, count, entries)) = parse_stsz(body) {
                    default_size = def;
                    sample_count = count;
                    sizes = entries;
                }
            }
            b"stco" => chunk_offs = parse_stco(body),
            b"co64" => chunk_offs = parse_co64(body),
            b"stss" => {
                has_stss = true;
                sync = parse_stss(body);
            }
            // `stz2` compact sizes: uncommon in Stage-1 FATE; ignored.
            _ => {}
        }
        pos += hdr.size;
    }

    if sample_count == 0 || chunk_offs.is_empty() || stsc.is_empty() || stts.is_empty() {
        return Vec::new();
    }

    if default_size != 0 {
        sizes = vec![default_size; sample_count as usize];
    } else if sizes.len() != sample_count as usize {
        return Vec::new();
    }

    let durations = expand_runs(&stts, sample_count as usize);
    if durations.len() != sample_count as usize {
        return Vec::new();
    }
    let composition_offsets = if ctts.is_empty() {
        vec![0i32; sample_count as usize]
    } else {
        let c = expand_runs_i32(&ctts, sample_count as usize);
        if c.len() != sample_count as usize {
            return Vec::new();
        }
        c
    };

    let offsets = chunk_sample_offsets(&stsc, &chunk_offs, &sizes);
    if offsets.len() != sample_count as usize {
        return Vec::new();
    }

    let sync_set: std::collections::BTreeSet<u32> = sync.into_iter().collect();
    let mut dts = 0u64;
    let mut out = Vec::with_capacity(sample_count as usize);
    for i in 0..sample_count as usize {
        let sample_number = (i as u32).saturating_add(1);
        let key = if has_stss {
            sync_set.contains(&sample_number)
        } else {
            true
        };
        let duration = durations[i];
        out.push(StblSample {
            offset: offsets[i],
            size: sizes[i],
            dts: dts as i64,
            duration,
            cto: composition_offsets[i],
            key,
            discard: false,
        });
        dts = dts.saturating_add(u64::from(duration));
    }
    out
}

fn parse_stts(body: &[u8]) -> Vec<(u32, u32)> {
    if body.len() < 8 {
        return Vec::new();
    }
    let count = u32::from_be_bytes([body[4], body[5], body[6], body[7]]) as usize;
    let mut out = Vec::with_capacity(count);
    let mut pos = 8;
    for _ in 0..count {
        if pos + 8 > body.len() {
            break;
        }
        let n = u32::from_be_bytes([body[pos], body[pos + 1], body[pos + 2], body[pos + 3]]);
        let delta =
            u32::from_be_bytes([body[pos + 4], body[pos + 5], body[pos + 6], body[pos + 7]]);
        out.push((n, delta));
        pos += 8;
    }
    out
}

fn parse_ctts(body: &[u8]) -> Vec<(u32, i32)> {
    if body.len() < 8 {
        return Vec::new();
    }
    let version = body[0];
    let count = u32::from_be_bytes([body[4], body[5], body[6], body[7]]) as usize;
    let mut out = Vec::with_capacity(count);
    let mut pos = 8;
    for _ in 0..count {
        if pos + 8 > body.len() {
            break;
        }
        let n = u32::from_be_bytes([body[pos], body[pos + 1], body[pos + 2], body[pos + 3]]);
        let raw = u32::from_be_bytes([body[pos + 4], body[pos + 5], body[pos + 6], body[pos + 7]]);
        let off = if version == 1 {
            raw as i32
        } else {
            // version 0: unsigned offset; fit into i32 for PTS math.
            i32::try_from(raw).unwrap_or(i32::MAX)
        };
        out.push((n, off));
        pos += 8;
    }
    out
}

fn parse_stsc(body: &[u8]) -> Vec<(u32, u32)> {
    if body.len() < 8 {
        return Vec::new();
    }
    let count = u32::from_be_bytes([body[4], body[5], body[6], body[7]]) as usize;
    let mut out = Vec::with_capacity(count);
    let mut pos = 8;
    for _ in 0..count {
        if pos + 12 > body.len() {
            break;
        }
        let first = u32::from_be_bytes([body[pos], body[pos + 1], body[pos + 2], body[pos + 3]]);
        let spc = u32::from_be_bytes([body[pos + 4], body[pos + 5], body[pos + 6], body[pos + 7]]);
        // sample_description_index at +8 ignored (single description).
        out.push((first, spc));
        pos += 12;
    }
    out
}

fn parse_stsz(body: &[u8]) -> Option<(u32, u32, Vec<u32>)> {
    if body.len() < 12 {
        return None;
    }
    let sample_size = u32::from_be_bytes([body[4], body[5], body[6], body[7]]);
    let sample_count = u32::from_be_bytes([body[8], body[9], body[10], body[11]]);
    if sample_size != 0 {
        return Some((sample_size, sample_count, Vec::new()));
    }
    let need = 12usize.saturating_add((sample_count as usize).saturating_mul(4));
    if body.len() < need {
        return None;
    }
    let mut entries = Vec::with_capacity(sample_count as usize);
    let mut pos = 12;
    for _ in 0..sample_count {
        entries.push(u32::from_be_bytes([
            body[pos],
            body[pos + 1],
            body[pos + 2],
            body[pos + 3],
        ]));
        pos += 4;
    }
    Some((0, sample_count, entries))
}

fn parse_stco(body: &[u8]) -> Vec<u64> {
    if body.len() < 8 {
        return Vec::new();
    }
    let count = u32::from_be_bytes([body[4], body[5], body[6], body[7]]) as usize;
    let mut out = Vec::with_capacity(count);
    let mut pos = 8;
    for _ in 0..count {
        if pos + 4 > body.len() {
            break;
        }
        out.push(u64::from(u32::from_be_bytes([
            body[pos],
            body[pos + 1],
            body[pos + 2],
            body[pos + 3],
        ])));
        pos += 4;
    }
    out
}

fn parse_co64(body: &[u8]) -> Vec<u64> {
    if body.len() < 8 {
        return Vec::new();
    }
    let count = u32::from_be_bytes([body[4], body[5], body[6], body[7]]) as usize;
    let mut out = Vec::with_capacity(count);
    let mut pos = 8;
    for _ in 0..count {
        if pos + 8 > body.len() {
            break;
        }
        out.push(u64::from_be_bytes([
            body[pos],
            body[pos + 1],
            body[pos + 2],
            body[pos + 3],
            body[pos + 4],
            body[pos + 5],
            body[pos + 6],
            body[pos + 7],
        ]));
        pos += 8;
    }
    out
}

fn parse_stss(body: &[u8]) -> Vec<u32> {
    if body.len() < 8 {
        return Vec::new();
    }
    let count = u32::from_be_bytes([body[4], body[5], body[6], body[7]]) as usize;
    let mut out = Vec::with_capacity(count);
    let mut pos = 8;
    for _ in 0..count {
        if pos + 4 > body.len() {
            break;
        }
        out.push(u32::from_be_bytes([
            body[pos],
            body[pos + 1],
            body[pos + 2],
            body[pos + 3],
        ]));
        pos += 4;
    }
    out
}

fn expand_runs(runs: &[(u32, u32)], total: usize) -> Vec<u32> {
    let mut out = Vec::with_capacity(total);
    for &(n, v) in runs {
        for _ in 0..n {
            if out.len() >= total {
                return out;
            }
            out.push(v);
        }
    }
    out
}

fn expand_runs_i32(runs: &[(u32, i32)], total: usize) -> Vec<i32> {
    let mut out = Vec::with_capacity(total);
    for &(n, v) in runs {
        for _ in 0..n {
            if out.len() >= total {
                return out;
            }
            out.push(v);
        }
    }
    out
}

/// Map each sample index to a file-absolute byte offset via `stsc` + chunk offsets.
fn chunk_sample_offsets(stsc: &[(u32, u32)], chunk_offs: &[u64], sizes: &[u32]) -> Vec<u64> {
    let mut out = Vec::with_capacity(sizes.len());
    let mut sample_idx = 0usize;
    for (entry_i, &(first_chunk, samples_per_chunk)) in stsc.iter().enumerate() {
        if samples_per_chunk == 0 {
            return Vec::new();
        }
        let next_first = stsc.get(entry_i + 1).map_or(u32::MAX, |(f, _)| *f);
        let start_chunk = first_chunk.saturating_sub(1) as usize;
        let end_chunk = if next_first == u32::MAX {
            chunk_offs.len()
        } else {
            next_first.saturating_sub(1) as usize
        };
        for &chunk_off in chunk_offs
            .iter()
            .take(end_chunk.min(chunk_offs.len()))
            .skip(start_chunk)
        {
            let mut off = chunk_off;
            for _ in 0..samples_per_chunk {
                if sample_idx >= sizes.len() {
                    return out;
                }
                out.push(off);
                off = off.saturating_add(u64::from(sizes[sample_idx]));
                sample_idx += 1;
            }
        }
    }
    out
}

/// Find a direct child box payload by type.
#[must_use]
pub fn find_child(parent: &[u8], typ: FourCc) -> Option<&[u8]> {
    let mut pos = 0;
    while pos + 8 <= parent.len() {
        let hdr = parse_header(&parent[pos..])?;
        if pos + hdr.size > parent.len() {
            return None;
        }
        if hdr.typ == typ {
            return Some(&parent[pos + hdr.header_len..pos + hdr.size]);
        }
        pos += hdr.size;
    }
    None
}
