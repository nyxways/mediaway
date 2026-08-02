//! Media Foundation runtime helpers (Windows only).

#![allow(unsafe_code)]

use std::sync::OnceLock;

use mediaway_encoder::EncodeError;
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
#[must_use]
pub(crate) fn to_hns(units: i64, time_base_num: u64, time_base_den: u32) -> i64 {
    if time_base_den == 0 {
        return 0;
    }
    let num = i128::from(units) * i128::from(time_base_num) * 10_000_000;
    let den = i128::from(time_base_den);
    i64::try_from(num / den).unwrap_or(0)
}
