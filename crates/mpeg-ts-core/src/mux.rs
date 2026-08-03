//! MPEG-TS mux: PAT/PMT + per-PID PES packetization over 188-byte TS packets.

#![forbid(unsafe_code)]

use smallvec::SmallVec;

use crate::error::Error;
use crate::packet::write_ts_packets;
use crate::pes::{build_pes_header, stream_id_for};
use crate::psi::{build_pat_section, build_pmt_section};
use crate::types::{ElementaryStream, StreamType};

const PAT_PID: u16 = 0;

const fn validate_pid(pid: u16) -> Result<(), Error> {
    if pid > 0x1FFF || pid == 0 || pid == 1 {
        Err(Error::InvalidPid(pid))
    } else {
        Ok(())
    }
}

/// Writes a single-program MPEG-TS stream: `write_pat_pmt` (call at the start,
/// and periodically — real players expect PAT/PMT to repeat) plus
/// `write_access_unit` per elementary-stream access unit.
#[derive(Debug)]
pub struct Muxer {
    program_number: u16,
    pmt_pid: u16,
    streams: SmallVec<[(ElementaryStream, u8); 4]>, // (stream, continuity_counter)
    pat_cc: u8,
    pmt_cc: u8,
}

impl Muxer {
    /// Start a mux session for one program. `pmt_pid` and every stream's `pid`
    /// must be in `2..=0x1FFF` (`0`/`1` are reserved for PAT/CAT).
    pub fn new(
        program_number: u16,
        pmt_pid: u16,
        streams: &[ElementaryStream],
    ) -> Result<Self, Error> {
        validate_pid(pmt_pid)?;
        for stream in streams {
            validate_pid(stream.pid)?;
        }
        Ok(Self {
            program_number,
            pmt_pid,
            streams: streams.iter().map(|s| (*s, 0u8)).collect(),
            pat_cc: 0,
            pmt_cc: 0,
        })
    }

    /// Write one PAT packet + one PMT packet describing this program's streams.
    pub fn write_pat_pmt(&mut self, out: &mut Vec<u8>) {
        let pat = build_pat_section(1, self.program_number, self.pmt_pid);
        write_ts_packets(
            out,
            PAT_PID,
            &mut self.pat_cc,
            &pusi_payload(&pat),
            true,
            false,
        );

        let elementary: SmallVec<[ElementaryStream; 4]> =
            self.streams.iter().map(|(s, _)| *s).collect();
        let pmt = build_pmt_section(self.program_number, &elementary);
        write_ts_packets(
            out,
            self.pmt_pid,
            &mut self.pmt_cc,
            &pusi_payload(&pmt),
            true,
            false,
        );
    }

    /// Packetize one access unit (already-encoded elementary-stream data, e.g.
    /// Annex-B H.264 or an ADTS AAC frame) into a PES packet and split it across
    /// TS packets on `pid`.
    pub fn write_access_unit(
        &mut self,
        pid: u16,
        data: &[u8],
        pts_90k: u64,
        dts_90k: Option<u64>,
        random_access: bool,
        out: &mut Vec<u8>,
    ) -> Result<(), Error> {
        let (stream, cc) = self
            .streams
            .iter_mut()
            .find(|(s, _)| s.pid == pid)
            .ok_or(Error::UnknownPid(pid))?;
        let is_video = matches!(stream.stream_type, StreamType::H264 | StreamType::Hevc);

        let mut payload = build_pes_header(stream_id_for(is_video), data.len(), pts_90k, dts_90k);
        payload.extend_from_slice(data);
        write_ts_packets(out, pid, cc, &payload, true, random_access);
        Ok(())
    }
}

fn pusi_payload(section: &[u8]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(1 + section.len());
    payload.push(0); // pointer_field = 0 (section starts immediately)
    payload.extend_from_slice(section);
    payload
}

#[cfg(test)]
#[path = "mux_tests.rs"]
mod tests;
