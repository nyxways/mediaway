//! DX11 Zero-Copy decode: hardware H.264 MFT + DXGI output surfaces.

#![allow(unsafe_code)]

use mediaway_common::NativeHandle;
use mediaway_decoder::DecodeError;
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11Multithread, ID3D11Texture2D};
use windows::Win32::Media::MediaFoundation::{
    IMFActivate, IMFDXGIBuffer, IMFDXGIDeviceManager, IMFMediaEventGenerator, IMFTransform,
    METransformHaveOutput, METransformNeedInput, MF_E_NO_EVENTS_AVAILABLE, MF_EVENT_FLAG_NO_WAIT,
    MF_SA_D3D11_AWARE, MF_TRANSFORM_ASYNC, MF_TRANSFORM_ASYNC_UNLOCK, MFCreateDXGIDeviceManager,
    MFMediaType_Video, MFT_CATEGORY_VIDEO_DECODER, MFT_ENUM_FLAG, MFT_ENUM_FLAG_HARDWARE,
    MFT_ENUM_FLAG_SORTANDFILTER, MFT_MESSAGE_SET_D3D_MANAGER, MFT_REGISTER_TYPE_INFO, MFTEnumEx,
    MFVideoFormat_NV12,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::core::{GUID, Interface};

use super::shared::{begin_streaming, configure_decode_types};

/// DXGI / async state for Zero-Copy decode sessions.
pub(super) struct Dx11Session {
    _device: ID3D11Device,
    _manager: IMFDXGIDeviceManager,
    events: Option<IMFMediaEventGenerator>,
    need_input: u32,
    pub(super) output_provides_samples: bool,
}

/// Open a hardware video decoder MFT bound to `device` for DXGI outputs.
pub(super) fn open_hw_decoder(
    device: ID3D11Device,
    width: u32,
    height: u32,
    extra_data: &mediaway_common::Bytes,
    input_subtype: &GUID,
) -> Result<(IMFTransform, Dx11Session), DecodeError> {
    enable_multithread(&device);

    let mut reset_token = 0u32;
    let mut manager: Option<IMFDXGIDeviceManager> = None;
    // SAFETY: MFCreateDXGIDeviceManager writes token + manager out-params.
    unsafe { MFCreateDXGIDeviceManager(&raw mut reset_token, &raw mut manager) }
        .map_err(|_| DecodeError::Backend)?;
    let manager = manager.ok_or(DecodeError::Backend)?;
    // SAFETY: ResetDevice associates our ID3D11Device with the manager.
    unsafe { manager.ResetDevice(&device, reset_token) }.map_err(|_| DecodeError::Backend)?;

    let transform = activate_hw_decoder(input_subtype)?;
    ensure_d3d11_aware(&transform)?;

    // SAFETY: SET_D3D_MANAGER takes the raw IUnknown of the DXGI manager.
    let manager_ptr = Interface::as_raw(&manager) as usize;
    unsafe {
        transform
            .ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, manager_ptr)
            .map_err(|_| DecodeError::Backend)?;
    }

    let events = unlock_async_if_needed(&transform)?;
    configure_decode_types(&transform, width, height, extra_data, input_subtype)?;
    begin_streaming(&transform)?;

    let out_info = unsafe { transform.GetOutputStreamInfo(0) }.map_err(|_| DecodeError::Backend)?;
    let output_provides_samples = (out_info.dwFlags
        & windows::Win32::Media::MediaFoundation::MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32)
        != 0;

    Ok((
        transform,
        Dx11Session {
            _device: device,
            _manager: manager,
            events,
            need_input: 0,
            output_provides_samples,
        },
    ))
}

/// Extract `ID3D11Texture2D` + subresource from a decoder output `IMFSample`.
pub(super) fn texture_from_output_sample(
    sample: &windows::Win32::Media::MediaFoundation::IMFSample,
) -> Result<(ID3D11Texture2D, u32), DecodeError> {
    // SAFETY: decoder output sample exposes a DXGI media buffer.
    let buffer = unsafe { sample.GetBufferByIndex(0) }.map_err(|_| DecodeError::Backend)?;
    let dxgi: IMFDXGIBuffer = buffer.cast().map_err(|_| DecodeError::Backend)?;
    let mut resource: *mut std::ffi::c_void = std::ptr::null_mut();
    // SAFETY: GetResource returns a DXGI surface / D3D11 texture for our device.
    unsafe {
        dxgi.GetResource(&ID3D11Texture2D::IID, &raw mut resource)
            .map_err(|_| DecodeError::Backend)?;
    }
    if resource.is_null() {
        return Err(DecodeError::Backend);
    }
    // SAFETY: resource is a valid COM pointer from GetResource; we take ownership.
    let texture = unsafe { ID3D11Texture2D::from_raw(resource.cast()) };
    let subresource = unsafe { dxgi.GetSubresourceIndex() }.map_err(|_| DecodeError::Backend)?;
    Ok((texture, subresource))
}

pub(super) fn wait_need_input(session: &mut Dx11Session) -> Result<(), DecodeError> {
    let Some(_) = session.events else {
        return Ok(());
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
    while session.need_input == 0 {
        if std::time::Instant::now() > deadline {
            return Err(DecodeError::Backend);
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

pub(super) fn drain_events_nonblocking(session: &mut Dx11Session) -> Result<(), DecodeError> {
    if session.events.is_some() {
        drain_events(session)?;
    }
    Ok(())
}

/// Adopt a caller `ID3D11Device*` (`AddRef`) for the decode session.
pub(super) fn device_from_handle(handle: NativeHandle) -> Result<ID3D11Device, DecodeError> {
    let raw = handle.get() as *mut std::ffi::c_void;
    // SAFETY: borrowed device pointer; clone AddRefs for our session ownership.
    let borrowed =
        unsafe { ID3D11Device::from_raw_borrowed(&raw) }.ok_or(DecodeError::InvalidInput)?;
    Ok(borrowed.clone()) // clone: COM AddRef for session-owned device handle
}

fn drain_events(session: &mut Dx11Session) -> Result<(), DecodeError> {
    let Some(events) = session.events.as_ref() else {
        return Ok(());
    };
    loop {
        // SAFETY: non-blocking GetEvent.
        let event = match unsafe { events.GetEvent(MF_EVENT_FLAG_NO_WAIT) } {
            Ok(e) => e,
            Err(e) if e.code() == MF_E_NO_EVENTS_AVAILABLE => break,
            Err(_) => return Err(DecodeError::Backend),
        };
        let ty = unsafe { event.GetType() }.map_err(|_| DecodeError::Backend)?;
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
            // Best-effort; decoding may still work if already protected.
            let _ = mt.SetMultithreadProtected(true);
        }
    }
}

fn ensure_d3d11_aware(transform: &IMFTransform) -> Result<(), DecodeError> {
    let attrs = unsafe { transform.GetAttributes() }.map_err(|_| DecodeError::Unsupported)?;
    let aware = unsafe { attrs.GetUINT32(&MF_SA_D3D11_AWARE) }.unwrap_or(0);
    if aware == 0 {
        return Err(DecodeError::Unsupported);
    }
    Ok(())
}

fn unlock_async_if_needed(
    transform: &IMFTransform,
) -> Result<Option<IMFMediaEventGenerator>, DecodeError> {
    let attrs = unsafe { transform.GetAttributes() }.map_err(|_| DecodeError::Backend)?;
    let is_async = unsafe { attrs.GetUINT32(&MF_TRANSFORM_ASYNC) }.unwrap_or(0) != 0;
    if !is_async {
        return Ok(None);
    }
    unsafe { attrs.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1) }.map_err(|_| DecodeError::Backend)?;
    let event_gen: IMFMediaEventGenerator = transform.cast().map_err(|_| DecodeError::Backend)?;
    Ok(Some(event_gen))
}

fn activate_hw_decoder(input_subtype: &GUID) -> Result<IMFTransform, DecodeError> {
    let input = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: *input_subtype,
    };
    let output = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_NV12,
    };
    let flags = MFT_ENUM_FLAG(MFT_ENUM_FLAG_HARDWARE.0 | MFT_ENUM_FLAG_SORTANDFILTER.0);
    let mut activates: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count = 0u32;
    unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_DECODER,
            flags,
            Some(std::ptr::from_ref(&input)),
            Some(std::ptr::from_ref(&output)),
            &raw mut activates,
            &raw mut count,
        )
    }
    .map_err(|_| DecodeError::Unsupported)?;

    if activates.is_null() || count == 0 {
        return Err(DecodeError::Unsupported);
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
    chosen.ok_or(DecodeError::Unsupported)
}
