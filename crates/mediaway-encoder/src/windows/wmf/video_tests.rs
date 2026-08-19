#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "unit tests"
)]

use windows::Win32::Media::MediaFoundation::{
    IMFActivate, MFMediaType_Video, MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG,
    MFT_ENUM_FLAG_HARDWARE, MFT_ENUM_FLAG_SORTANDFILTER, MFT_FRIENDLY_NAME_Attribute,
    MFT_REGISTER_TYPE_INFO, MFTEnumEx, MFVideoFormat_AV1, MFVideoFormat_HEVC, MFVideoFormat_VP90,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::core::PWSTR;

/// Real `MFTEnumEx(MFT_CATEGORY_VIDEO_ENCODER, …)` results for HEVC / AV1 / VP9 on this
/// machine, both unfiltered (`MFT_ENUM_FLAG_SORTANDFILTER` only — any encoder MFT, HW or SW,
/// that declares the subtype, mirroring `activate_encoder_mft(_, false)`'s CPU-open call
/// shape) and `MFT_ENUM_FLAG_HARDWARE`-filtered (mirroring `activate_encoder_mft(_,
/// true)`'s DX11-open call shape). Informational: records real findings either way rather
/// than asserting a specific outcome, since which encoder MFTs are registered is a property
/// of the OS/driver install, not this crate — mirrors
/// `mediaway-decoder-windows`'s own `list_decoder_mfts_for_each_codec` doc-comment stance.
/// See `docs/roadmap.md` for the findings this produced on the verification host.
#[test]
fn list_encoder_mfts_for_each_codec() {
    super::super::runtime::ensure_mf().expect("MF runtime init");
    for (name, subtype) in [
        ("HEVC", MFVideoFormat_HEVC),
        ("AV1", MFVideoFormat_AV1),
        ("VP9", MFVideoFormat_VP90),
    ] {
        let unfiltered = enum_encoder_mft_names(subtype, false);
        let hw_only = enum_encoder_mft_names(subtype, true);
        eprintln!("{name}: any-flag encoder MFTs = {unfiltered:?}");
        eprintln!("{name}: MFT_ENUM_FLAG_HARDWARE encoder MFTs = {hw_only:?}");
    }
}

/// Real `MFTEnumEx` call + friendly-name lookup for every registered
/// `MFT_CATEGORY_VIDEO_ENCODER` MFT that declares `subtype` as an accepted output.
/// `hardware_only` mirrors `activate_encoder_mft`'s own two flag/input-filter shapes: the
/// hardware path passes no input-type filter (live-recorder pattern, DX11 Zero-Copy open),
/// the non-hardware path filters on NV12 input (CPU-upload open).
fn enum_encoder_mft_names(subtype: windows::core::GUID, hardware_only: bool) -> Vec<String> {
    let output = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: subtype,
    };
    let flags = if hardware_only {
        MFT_ENUM_FLAG(MFT_ENUM_FLAG_HARDWARE.0 | MFT_ENUM_FLAG_SORTANDFILTER.0)
    } else {
        MFT_ENUM_FLAG_SORTANDFILTER
    };
    let mut activates: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count = 0u32;
    // SAFETY: MFTEnumEx writes an activate-object array + count as out-params; freed below.
    let hr = unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            flags,
            None,
            Some(std::ptr::from_ref(&output)),
            &raw mut activates,
            &raw mut count,
        )
    };
    if hr.is_err() || activates.is_null() {
        return Vec::new();
    }
    let mut names = Vec::new();
    for i in 0..count as usize {
        // SAFETY: `activates` holds `count` valid `Option<IMFActivate>` slots from MFTEnumEx.
        let activate = unsafe { (*activates.add(i)).take() };
        if let Some(activate) = activate {
            names.push(friendly_name(&activate).unwrap_or_else(|| "<unnamed>".to_owned()));
        }
    }
    // SAFETY: `activates` was allocated by MFTEnumEx (CoTaskMemAlloc); we own and free it.
    unsafe {
        CoTaskMemFree(Some(activates.cast_const().cast()));
    }
    names
}

fn friendly_name(activate: &IMFActivate) -> Option<String> {
    let mut raw = PWSTR::null();
    let mut len = 0u32;
    // SAFETY: out-params written on success; the string is `CoTaskMemAlloc`'d and freed below.
    unsafe {
        activate.GetAllocatedString(&MFT_FRIENDLY_NAME_Attribute, &raw mut raw, &raw mut len)
    }
    .ok()?;
    if raw.is_null() {
        return None;
    }
    // SAFETY: `raw` is a valid null-terminated wide string per `GetAllocatedString`'s
    // contract, still valid at this point (freed only below).
    let name = unsafe { raw.to_string() }.ok();
    // SAFETY: matching `CoTaskMemFree` for the successful `GetAllocatedString` above.
    unsafe {
        CoTaskMemFree(Some(raw.0.cast()));
    }
    name
}
