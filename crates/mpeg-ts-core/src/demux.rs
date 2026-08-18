//! MPEG-TS demux: PAT/PMT tracking + per-PID PES reassembly from 188-byte TS
//! packets.
//!
//! PSI (PAT/PMT) sections spanning more than one TS packet are not
//! reassembled (v1 scope, crate-local ADR-0001) — this crate's own `Muxer`
//! never produces one (a single program with a handful of streams always fits
//! in one packet), but an arbitrary third-party multi-program stream with a
//! very large PMT would not parse correctly here.

#![forbid(unsafe_code)]

use std::collections::{HashMap, VecDeque};

use bytes::Bytes;
use smallvec::SmallVec;

use crate::error::Error;
use crate::packet::{PACKET_LEN, parse_ts_packet};
use crate::pes::parse_pes_header;
use crate::psi::{parse_pat_section, parse_pmt_section};
use crate::types::{AccessUnit, ElementaryStream};

const PAT_PID: u16 = 0;

#[derive(Debug, Default)]
struct PesAccumulator {
    data: Vec<u8>,
    random_access: bool,
}

/// Reads TS packets from pushed byte chunks, tracks PAT/PMT, and reassembles
/// per-PID PES packets into [`AccessUnit`]s.
#[derive(Debug, Default)]
pub struct Demuxer {
    buf: Vec<u8>,
    pmt_pid: Option<u16>,
    streams: SmallVec<[ElementaryStream; 4]>,
    accumulators: HashMap<u16, PesAccumulator>,
    pending: VecDeque<AccessUnit>,
}

impl Demuxer {
    /// New, empty demux session.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append incoming bytes (need not be 188-byte aligned across calls).
    pub fn push_bytes(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    /// Elementary streams from the most recently parsed PMT (empty until then).
    #[must_use]
    pub fn streams(&self) -> &[ElementaryStream] {
        &self.streams
    }

    /// Pop the next fully reassembled access unit, or `Ok(None)` if not enough
    /// bytes are buffered yet.
    ///
    /// An access unit is only confirmed complete once the *next* PES packet on
    /// the same PID starts (or [`Demuxer::finish`] is called) — this is
    /// inherent to how PES packetization signals its own boundaries, not a
    /// limitation specific to this crate.
    pub fn poll_access_unit(&mut self) -> Result<Option<AccessUnit>, Error> {
        loop {
            if let Some(unit) = self.pending.pop_front() {
                return Ok(Some(unit));
            }
            if !self.consume_one_packet()? {
                return Ok(None);
            }
        }
    }

    /// Force-emit whatever is still accumulating per PID as a final access
    /// unit each — call once at the end of a stream to avoid losing the very
    /// last access unit per PID (see [`Demuxer::poll_access_unit`]'s doc).
    pub fn finish(&mut self) -> Vec<AccessUnit> {
        let mut out = Vec::new();
        for (pid, acc) in self.accumulators.drain() {
            if acc.data.is_empty() {
                continue;
            }
            if let Ok(unit) = finish_pes(pid, &acc) {
                out.push(unit);
            }
        }
        out
    }

    fn consume_one_packet(&mut self) -> Result<bool, Error> {
        if self.buf.len() < PACKET_LEN {
            return Ok(false);
        }
        let packet_owned = self.buf[..PACKET_LEN].to_vec();
        let parsed = parse_ts_packet(&packet_owned)?;
        let pid = parsed.pid;

        if pid == PAT_PID {
            if parsed.pusi {
                let pat = parse_pat_section(parsed.payload)?;
                self.pmt_pid = Some(pat.pmt_pid);
            }
        } else if Some(pid) == self.pmt_pid {
            if parsed.pusi {
                let pmt = parse_pmt_section(parsed.payload)?;
                self.streams = pmt.streams;
            }
        } else if self.streams.iter().any(|s| s.pid == pid) {
            self.feed_pes(pid, parsed.pusi, parsed.random_access, parsed.payload)?;
        }

        self.buf.drain(0..PACKET_LEN);
        Ok(true)
    }

    fn feed_pes(
        &mut self,
        pid: u16,
        pusi: bool,
        random_access: bool,
        payload: &[u8],
    ) -> Result<(), Error> {
        if pusi {
            if let Some(prev) = self.accumulators.get(&pid)
                && !prev.data.is_empty()
            {
                let unit = finish_pes(pid, prev)?;
                self.pending.push_back(unit);
            }
            self.accumulators.insert(
                pid,
                PesAccumulator {
                    data: payload.to_vec(),
                    random_access,
                },
            );
        } else if let Some(acc) = self.accumulators.get_mut(&pid) {
            acc.data.extend_from_slice(payload);
        }
        Ok(())
    }
}

fn finish_pes(pid: u16, acc: &PesAccumulator) -> Result<AccessUnit, Error> {
    let header = parse_pes_header(&acc.data)?;
    Ok(AccessUnit {
        pid,
        data: Bytes::copy_from_slice(&acc.data[header.header_len..]),
        pts_90k: header.pts_90k,
        dts_90k: header.dts_90k,
        random_access: acc.random_access,
    })
}

#[cfg(test)]
#[path = "demux_tests.rs"]
mod tests;
