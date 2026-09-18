//! Media Foundation runtime helpers (Windows only).

#![allow(unsafe_code)]

use std::sync::OnceLock;

use crate::DecodeError;
use windows::Win32::Media::MediaFoundation::{MF_VERSION, MFSTARTUP_FULL, MFStartup};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};

static MF_INIT: OnceLock<Result<(), DecodeError>> = OnceLock::new();

/// `RPC_E_CHANGED_MODE` — COM already initialized with a different apartment.
#[allow(
    clippy::cast_possible_wrap,
    reason = "HRESULT bit pattern 0x80010106 as i32"
)]
const RPC_E_CHANGED_MODE: i32 = 0x8001_0106_u32 as i32;

/// Ensure COM + MF are initialized for this process (idempotent).
pub(crate) fn ensure_mf() -> Result<(), DecodeError> {
    MF_INIT
        .get_or_init(|| {
            // SAFETY: COINIT_MULTITHREADED is process-wide; RPC_E_CHANGED_MODE is OK if
            // the host already initialized COM differently.
            let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            if hr.is_err() && hr.0 != RPC_E_CHANGED_MODE {
                return Err(DecodeError::Backend);
            }
            // SAFETY: MFStartup is refcounted; we never call MFShutdown (process lifetime).
            unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }.map_err(|_| DecodeError::Backend)
        })
        .clone()
}

/// Unpack `high:low` from an MF `UINT64` frame size / rate attribute.
#[must_use]
pub(crate) fn unpack_u32_pair(packed: u64) -> (u32, u32) {
    let width = u32::try_from(packed >> 32).unwrap_or(0);
    let height = u32::try_from(packed & u64::from(u32::MAX)).unwrap_or(0);
    (width, height)
}

/// Pack `high:low` the way MF stores frame size / rate in a `UINT64`.
#[must_use]
pub(crate) const fn pack_u32_pair(high: u32, low: u32) -> u64 {
    ((high as u64) << 32) | (low as u64)
}

/// Convert a timestamp in `time_base` units to MF 100-nanosecond units.
///
/// Truncates toward zero; [`from_hns`] rounds to the nearest tick so the two are an exact
/// inverse. See that function for why the round trip has to be one.
#[must_use]
pub(crate) fn to_hns(units: i64, time_base_num: u64, time_base_den: u32) -> i64 {
    if time_base_den == 0 {
        return 0;
    }
    let num = i128::from(units) * i128::from(time_base_num) * 10_000_000;
    let den = i128::from(time_base_den);
    i64::try_from(num / den).unwrap_or(0)
}

/// Convert MF 100-nanosecond units back to `time_base` units.
///
/// # Why this rounds to the nearest tick rather than truncating
///
/// A packet timestamp makes the trip `tick → hns → tick` across the MFT: [`to_hns`] puts it on
/// the input sample, this reads it back off the output frame. If the two directions are not an
/// exact inverse, distinct input ticks come back as the same tick, and frames that were
/// ordered arrive claiming one instant.
///
/// Truncating both ways does that, because 10 000 000 is not divisible by most timebase
/// denominators. At `1/60` only every third tick survives:
///
/// | tick | `to_hns` | truncating back | nearest |
/// |---|---|---|---|
/// | 6 | 1 000 000 | 6 | 6 |
/// | 7 | 1 166 666 | **6** | 7 |
/// | 8 | 1 333 333 | **7** | 8 |
///
/// Found 2026-09-18 in `mediaway-encoder`'s copy of this pair, where it had put 279 video
/// packets and only 215 distinct timestamps into a real recording; this crate had the identical
/// defect and is fixed the same way. `1/30`, `1/24` and `1001/30000` are affected too.
///
/// # Why *nearest*, and not "away from zero"
///
/// Away-from-zero is also an exact inverse of [`to_hns`] on paper, and the argument for it is
/// neater: `to_hns` truncates toward zero, so dividing back always lands inside the tick below,
/// and rounding to the far end recovers the original.
///
/// It is wrong against a real MFT, which is what the encoder's `wmf_timestamp_round_trip`
/// integration test showed. **An MFT does not hand back the hns value that was written to it**
/// — it recomputes sample times with rounding of its own, landing a fraction of a tick above
/// the exact value as often as below. Away-from-zero pushes every such value into the next
/// tick; on the encode side that shifted a whole 30-frame sequence by one frame.
///
/// Nearest is right for both sources at once: within a fraction of a hns (a [`to_hns`] result)
/// or within a fraction of a tick (an MFT's own), the nearest tick is the intended one.
///
/// Exactness on the round trip holds whenever a tick is worth at least two hns
/// (`time_base_den <= num * 5 * 10^6`); past that the timebase is finer than MF can represent
/// and the precision is already gone in [`to_hns`], before this function sees it.
#[must_use]
pub(crate) fn from_hns(hns: i64, time_base_num: u64, time_base_den: u32) -> i64 {
    if time_base_num == 0 || time_base_den == 0 {
        return 0;
    }
    let num = i128::from(hns) * i128::from(time_base_den);
    // Positive by construction, so `num`'s sign alone decides which way lies away from zero.
    let den = i128::from(time_base_num) * 10_000_000;
    let quotient = num / den;
    // `%` keeps the sign of `num`, hence the `abs`. Ties round away from zero, matching the
    // usual convention; at these magnitudes an exact half is vanishingly rare either way.
    let rounded = if (num % den).abs() * 2 >= den {
        if num.is_negative() {
            quotient - 1
        } else {
            quotient + 1
        }
    } else {
        quotient
    };
    i64::try_from(rounded).unwrap_or(0)
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
