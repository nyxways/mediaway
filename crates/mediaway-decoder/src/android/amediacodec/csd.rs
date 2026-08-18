//! Split `VideoDecoderConfig::extra_data` into `csd-0`/`csd-1` (SPS/PPS) buffers for
//! `AMediaFormat_setBuffer`, re-prepending the Annex-B start code
//! [`mediaway_sw::h264::split_annex_b`] strips off.
//!
//! Pure byte-framing logic, independent of a real `MediaCodec`/`AMediaFormat` session so it is
//! unit-testable without an Android device or NDK — the caller ([`super::video`]) forwards the
//! result straight to `MediaFormat::set_buffer("csd-0"/"csd-1", …)` at `configure()` time.
//!
//! **Real detail** (ADR android/0001 § Decision): [`split_annex_b`] returns NAL bytes
//! **without** the start code (`content_begin = pair[0] + 3`, i.e. the returned slice begins at
//! the NAL header byte) — so a naive "split and forward" would hand `AMediaCodec` bare NAL
//! bytes, not the start-code-prefixed buffers the documented convention calls for. This module
//! re-prepends a canonical 4-byte `00 00 00 01` start code to each split SPS/PPS slice before
//! it is handed to `AMediaFormat::set_buffer`. This crate does not parse the SPS/PPS RBSP for
//! decode purposes — the NAL split is a byte-level framing operation only; `AMediaCodec`
//! remains a black box past this framing step.

use mediaway_sw::h264::{NalUnit, NalUnitType, split_annex_b};

/// Prepend a canonical 4-byte Annex-B start code (`00 00 00 01`) to `nal` (a NAL-header-through-
/// payload slice as returned by [`split_annex_b`], with `emulation_prevention_three_byte`
/// bytes still intact — this is a byte-framing operation, not a bitstream rewrite).
pub(super) fn prepend_start_code(nal: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + nal.len());
    out.extend_from_slice(&[0, 0, 0, 1]);
    out.extend_from_slice(nal);
    out
}

/// Split `extra_data` (Annex-B framed) and return the first SPS (`csd-0`) / PPS (`csd-1`) NAL
/// found, each with a start code re-prepended and ready for `AMediaFormat::set_buffer`.
///
/// Best-effort, not required: [`VideoDecoderConfig::extra_data`](crate::VideoDecoderConfig)
/// "may be empty until first keyframe", and `AMediaCodec` decoders documented-ly accept
/// in-band SPS/PPS from the first pushed packet's own NAL units too — see ADR android/0001 §
/// Decision. Returns `(None, None)` if `extra_data` has no start code, or `None` per slot when
/// no SPS/PPS NAL is found. If `extra_data` contains more than one SPS or PPS (rare but legal),
/// only the first of each is returned — an unhandled edge case this stage (ADR android/0001 §
/// Consequences).
pub(super) fn split_csd(extra_data: &[u8]) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    let Ok(nals) = split_annex_b(extra_data) else {
        return (None, None);
    };
    let mut sps = None;
    let mut pps = None;
    for nal in nals {
        let Ok(unit) = NalUnit::parse(nal) else {
            continue;
        };
        match unit.unit_type {
            NalUnitType::Sps if sps.is_none() => sps = Some(prepend_start_code(nal)),
            NalUnitType::Pps if pps.is_none() => pps = Some(prepend_start_code(nal)),
            _ => {}
        }
        if sps.is_some() && pps.is_some() {
            break;
        }
    }
    (sps, pps)
}

#[cfg(test)]
#[path = "csd_tests.rs"]
mod tests;
