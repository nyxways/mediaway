//! DX11 Zero-Copy input: hardware video encoder MFT + DXGI surface buffers.

#![allow(unsafe_code)]

use crate::EncodeError;
use mediaway_common::NativeHandle;
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11Multithread, ID3D11Texture2D};
use windows::Win32::Media::MediaFoundation::{
    IMFActivate, IMFDXGIDeviceManager, IMFMediaEventGenerator, IMFSample, IMFTransform,
    METransformHaveOutput, METransformNeedInput, MF_E_NO_EVENTS_AVAILABLE, MF_EVENT_FLAG_NO_WAIT,
    MF_SA_D3D11_AWARE, MF_TRANSFORM_ASYNC, MF_TRANSFORM_ASYNC_UNLOCK, MFCreateDXGIDeviceManager,
    MFCreateDXGISurfaceBuffer, MFCreateSample, MFMediaType_Video, MFT_CATEGORY_VIDEO_ENCODER,
    MFT_ENUM_FLAG, MFT_ENUM_FLAG_HARDWARE, MFT_ENUM_FLAG_SORTANDFILTER,
    MFT_MESSAGE_SET_D3D_MANAGER, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES, MFT_REGISTER_TYPE_INFO,
    MFTEnumEx, MFVideoFormat_NV12,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::core::{GUID, Interface};

use super::runtime::to_hns;
use super::shared::{begin_streaming, bitrate_and_fps, configure_types, output_buffer_hint};

/// DXGI / async state for Zero-Copy sessions.
pub(super) struct Dx11Session {
    _device: ID3D11Device,
    _manager: IMFDXGIDeviceManager,
    events: Option<IMFMediaEventGenerator>,
    need_input: u32,
    pub(super) output_provides_samples: bool,
}

/// Open a hardware video encoder MFT bound to `device` for DXGI inputs.
pub(super) fn open_hw_encoder(
    device: ID3D11Device,
    width: u32,
    height: u32,
    time_base_num: u64,
    time_base_den: u32,
    bitrate_bps: u32,
    output_subtype: &GUID,
    input_pixel_format: mediaway_common::PixelFormat,
) -> Result<(IMFTransform, Dx11Session, u32), EncodeError> {
    enable_multithread(&device);

    let mut reset_token = 0u32;
    let mut manager: Option<IMFDXGIDeviceManager> = None;
    // SAFETY: MFCreateDXGIDeviceManager writes token + manager out-params.
    unsafe { MFCreateDXGIDeviceManager(&raw mut reset_token, &raw mut manager) }
        .map_err(|_| EncodeError::Backend)?;
    let manager = manager.ok_or(EncodeError::Backend)?;
    // SAFETY: ResetDevice associates our ID3D11Device with the manager.
    unsafe { manager.ResetDevice(&device, reset_token) }.map_err(|_| EncodeError::Backend)?;

    let transform = activate_hw_encoder(output_subtype)?;
    ensure_d3d11_aware(&transform)?;

    // SAFETY: SET_D3D_MANAGER takes the raw IUnknown of the DXGI manager.
    let manager_ptr = Interface::as_raw(&manager) as usize;
    unsafe {
        transform
            .ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, manager_ptr)
            .map_err(|_| EncodeError::Backend)?;
    }

    let events = unlock_async_if_needed(&transform)?;
    let (bitrate, fps_num, fps_den) = bitrate_and_fps(bitrate_bps, time_base_num, time_base_den);
    configure_types(
        &transform,
        width,
        height,
        fps_num,
        fps_den,
        bitrate,
        output_subtype,
        input_pixel_format,
    )?;
    begin_streaming(&transform)?;

    let out_info = unsafe { transform.GetOutputStreamInfo(0) }.map_err(|_| EncodeError::Backend)?;
    let output_provides_samples =
        (out_info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32) != 0;
    let output_buf_size = output_buffer_hint(&transform)?;

    Ok((
        transform,
        Dx11Session {
            _device: device,
            _manager: manager,
            events,
            need_input: 0,
            output_provides_samples,
        },
        output_buf_size,
    ))
}

/// Wrap a caller-owned `ID3D11Texture2D*` as an MF sample (no CPU copy).
pub(super) fn sample_from_dx11_texture(
    texture: NativeHandle,
    subresource: u32,
    pts: i64,
    duration: u64,
    time_base_num: u64,
    time_base_den: u32,
) -> Result<IMFSample, EncodeError> {
    let raw = texture.get() as *mut std::ffi::c_void;
    // SAFETY: caller guarantees a live texture pointer for the duration of ProcessInput.
    let texture =
        unsafe { ID3D11Texture2D::from_raw_borrowed(&raw) }.ok_or(EncodeError::InvalidInput)?;
    let buffer =
        unsafe { MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, texture, subresource, false) }
            .map_err(|_| EncodeError::Backend)?;

    let sample: IMFSample = unsafe { MFCreateSample() }.map_err(|_| EncodeError::Backend)?;
    unsafe { sample.AddBuffer(&buffer) }.map_err(|_| EncodeError::Backend)?;

    let hns = to_hns(pts, time_base_num, time_base_den);
    let dur = to_hns(
        i64::try_from(duration).unwrap_or(0),
        time_base_num,
        time_base_den,
    )
    .max(1);
    unsafe {
        sample
            .SetSampleTime(hns)
            .map_err(|_| EncodeError::Backend)?;
        sample
            .SetSampleDuration(dur)
            .map_err(|_| EncodeError::Backend)?;
    }
    Ok(sample)
}

pub(super) fn wait_need_input(session: &mut Dx11Session) -> Result<(), EncodeError> {
    let Some(_) = session.events else {
        return Ok(());
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
    while session.need_input == 0 {
        if std::time::Instant::now() > deadline {
            return Err(EncodeError::Backend);
        }
        drain_events(session)?;
        if session.need_input > 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    Ok(())
}

#[allow(
    clippy::missing_const_for_fn,
    reason = "mutates session; kept non-const for API symmetry"
)]
pub(super) fn consume_need_input(session: &mut Dx11Session) {
    if session.events.is_some() {
        session.need_input = session.need_input.saturating_sub(1);
    }
}

pub(super) fn drain_events_nonblocking(session: &mut Dx11Session) -> Result<(), EncodeError> {
    if session.events.is_some() {
        drain_events(session)?;
    }
    Ok(())
}

/// Adopt a caller `ID3D11Device*` (`AddRef`) for the encode session.
pub(super) fn device_from_handle(handle: NativeHandle) -> Result<ID3D11Device, EncodeError> {
    let raw = handle.get() as *mut std::ffi::c_void;
    // SAFETY: borrowed device pointer; clone AddRefs for our session ownership.
    let borrowed =
        unsafe { ID3D11Device::from_raw_borrowed(&raw) }.ok_or(EncodeError::InvalidInput)?;
    Ok(borrowed.clone()) // clone: COM AddRef for session-owned device handle
}

fn drain_events(session: &mut Dx11Session) -> Result<(), EncodeError> {
    let Some(events) = session.events.as_ref() else {
        return Ok(());
    };
    loop {
        // SAFETY: non-blocking GetEvent.
        let event = match unsafe { events.GetEvent(MF_EVENT_FLAG_NO_WAIT) } {
            Ok(e) => e,
            Err(e) if e.code() == MF_E_NO_EVENTS_AVAILABLE => break,
            Err(_) => return Err(EncodeError::Backend),
        };
        let ty = unsafe { event.GetType() }.map_err(|_| EncodeError::Backend)?;
        if ty == METransformNeedInput.0 as u32 {
            session.need_input = session.need_input.saturating_add(1);
        } else if ty == METransformHaveOutput.0 as u32 {
            // ProcessOutput drained by caller.
        }
    }
    Ok(())
}

fn enable_multithread(device: &ID3D11Device) {
    if let Ok(mt) = device.cast::<ID3D11Multithread>() {
        unsafe {
            // Best-effort; encoding may still work if already protected.
            let _ = mt.SetMultithreadProtected(true);
        }
    }
}

fn ensure_d3d11_aware(transform: &IMFTransform) -> Result<(), EncodeError> {
    let attrs = unsafe { transform.GetAttributes() }.map_err(|_| EncodeError::Unsupported)?;
    let aware = unsafe { attrs.GetUINT32(&MF_SA_D3D11_AWARE) }.unwrap_or(0);
    if aware == 0 {
        return Err(EncodeError::Unsupported);
    }
    Ok(())
}

fn unlock_async_if_needed(
    transform: &IMFTransform,
) -> Result<Option<IMFMediaEventGenerator>, EncodeError> {
    let attrs = unsafe { transform.GetAttributes() }.map_err(|_| EncodeError::Backend)?;
    let is_async = unsafe { attrs.GetUINT32(&MF_TRANSFORM_ASYNC) }.unwrap_or(0) != 0;
    if !is_async {
        return Ok(None);
    }
    unsafe { attrs.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1) }.map_err(|_| EncodeError::Backend)?;
    let event_gen: IMFMediaEventGenerator = transform.cast().map_err(|_| EncodeError::Backend)?;
    Ok(Some(event_gen))
}

fn activate_hw_encoder(output_subtype: &GUID) -> Result<IMFTransform, EncodeError> {
    activate_encoder_mft(output_subtype, true)
}

/// Enumerate an encoder MFT for → `output_subtype`.
///
/// Hardware path passes **no input type filter** (live-recorder pattern) so MFTs that
/// accept ARGB32 or NV12 both appear; input subtype is chosen in [`configure_types`].
pub(super) fn activate_encoder_mft(
    output_subtype: &GUID,
    hardware_only: bool,
) -> Result<IMFTransform, EncodeError> {
    let output = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: *output_subtype,
    };
    let flags = if hardware_only {
        MFT_ENUM_FLAG(MFT_ENUM_FLAG_HARDWARE.0 | MFT_ENUM_FLAG_SORTANDFILTER.0)
    } else {
        MFT_ENUM_FLAG_SORTANDFILTER
    };
    let mut activates: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count = 0u32;
    let input_nv12 = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_NV12,
    };
    // Soft/CPU path: keep NV12 filter (inbox H.264). HW path: no input filter.
    let input_ptr = if hardware_only {
        None
    } else {
        Some(std::ptr::from_ref(&input_nv12))
    };
    unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            flags,
            input_ptr,
            Some(std::ptr::from_ref(&output)),
            &raw mut activates,
            &raw mut count,
        )
    }
    .map_err(|_| EncodeError::Unsupported)?;

    if activates.is_null() || count == 0 {
        return Err(EncodeError::Unsupported);
    }

    let mut chosen: Option<IMFTransform> = None;
    for i in 0..count as usize {
        let activate = unsafe { (*activates.add(i)).take() };
        let Some(activate) = activate else {
            continue;
        };
        if let Ok(t) = unsafe { activate.ActivateObject::<IMFTransform>() } {
            chosen = Some(t);
            break;
        }
    }
    unsafe {
        CoTaskMemFree(Some(activates as *const _));
    }
    chosen.ok_or(EncodeError::Unsupported)
}
