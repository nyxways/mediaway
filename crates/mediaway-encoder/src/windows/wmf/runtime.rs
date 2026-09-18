//! Media Foundation runtime helpers (Windows only).

#![allow(unsafe_code)]

use std::sync::OnceLock;

use crate::EncodeError;
use windows::Win32::Media::MediaFoundation::{MF_VERSION, MFSTARTUP_FULL, MFStartup};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};

static MF_INIT: OnceLock<Result<(), EncodeError>> = OnceLock::new();

/// `RPC_E_CHANGED_MODE` — COM already initialized with a different apartment.
#[allow(
    clippy::cast_possible_wrap,
    reason = "HRESULT bit pattern 0x80010106 as i32"
)]
const RPC_E_CHANGED_MODE: i32 = 0x8001_0106_u32 as i32;

/// Ensure COM + MF are initialized for this process (idempotent).
pub(crate) fn ensure_mf() -> Result<(), EncodeError> {
    MF_INIT
        .get_or_init(|| {
            // SAFETY: COINIT_MULTITHREADED is process-wide; RPC_E_CHANGED_MODE is OK if
            // the host already initialized COM differently.
            let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            if hr.is_err() && hr.0 != RPC_E_CHANGED_MODE {
                return Err(EncodeError::Backend);
            }
            // SAFETY: MFStartup is refcounted; we never call MFShutdown (process lifetime).
            unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }.map_err(|_| EncodeError::Backend)
        })
        .clone()
}

/// Pack `high:low` the way MF stores frame size / rate in a `UINT64`.
#[must_use]
pub(crate) const fn pack_u32_pair(high: u32, low: u32) -> u64 {
    ((high as u64) << 32) | (low as u64)
}

/// Convert a timestamp in `time_base` units to MF 100-nanosecond units.
///
/// Truncates toward zero. [`from_hns`] rounds to the nearest tick, which is what makes the pair
/// an exact inverse; that function documents why the round trip has to be one.
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
/// Every timestamp makes the trip `tick → hns → tick` on its way through an MFT: it goes in on
/// the input sample and comes back on the output one. Unless the two directions are an exact
/// inverse, *distinct* input ticks come back as the *same* tick — and a video track with two
/// frames claiming one instant is malformed, however well it happens to play.
///
/// Truncating both ways does exactly that, because 10 000 000 is not divisible by most timebase
/// denominators. At `1/60`, only every third tick survives:
///
/// | tick | `to_hns` | truncating back | nearest |
/// |---|---|---|---|
/// | 6 | 1 000 000 | 6 | 6 |
/// | 7 | 1 166 666 | **6** | 7 |
/// | 8 | 1 333 333 | **7** | 8 |
///
/// Measured 2026-09-18 on a real recording taken through this path: 279 video packets carried
/// only 215 distinct presentation timestamps. Decoding it, ffmpeg warned *"Application provided
/// invalid, non monotonically increasing dts to muxer"* — the duplicate presentation times,
/// surfacing as duplicate timestamps on its decoded output. The file's own decode timestamps
/// were monotonic throughout. `1/30`, `1/24` and `1001/30000` are all affected the same way,
/// and so is audio — `aac.rs` shares [`to_hns`].
///
/// # Why *nearest*, and not "away from zero"
///
/// Away-from-zero is also an exact inverse of [`to_hns`]: `to_hns` truncates toward zero, so
/// dividing back lands just below the original tick, and the far end recovers it. On every
/// value measured the two rules agree. The HEVC MFT hands back exactly the hns written to it;
/// the H.264 MFT recomputes its own, landing at or a hair *below* each whole tick. No MFT was
/// observed rounding *up*.
///
/// Nearest is chosen for the case that was not observed. An MFT that computed its sample
/// times by rounding up would land a hair above a whole tick, and away-from-zero would push
/// that value into the next tick, while nearest still returns the intended one. For every
/// value actually seen, the two rules cost the same. This is a design argument, not a
/// measurement. An earlier version of this comment claimed a measurement, which was wrong;
/// ADR-0013 § Correction records it.
///
/// Exactness on the round trip holds whenever a tick is worth at least two hns, i.e.
/// `time_base_den <= num * 5 * 10^6`. Past that the timebase is finer than MF's own resolution
/// and the precision is already gone inside [`to_hns`], before this function sees it — no
/// rounding rule here can recover it.
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
