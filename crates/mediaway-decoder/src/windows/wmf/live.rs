//! Live H.264 decode: DXVA hardware decode through Media Foundation into D3D11 NV12 textures,
//! tuned for an interactive stream (one frame out per frame in, loss recovery, size changes).
//!
//! **Which decoder.** Windows has no vendor "hardware" H.264 decoder MFT on NVIDIA (and on
//! most machines): `MFT_ENUM_FLAG_HARDWARE` finds nothing, which is why the general DX11
//! path ([`super::h264`], ADR-0001) is `Unsupported` on the reference RTX 4090. GPU decode of
//! H.264 goes through the inbox *synchronous* "Microsoft H264 Video Decoder MFT": given a D3D11
//! device manager (`MFT_MESSAGE_SET_D3D_MANAGER`) it decodes with DXVA on that device and hands
//! out samples backed by its own D3D11 texture array. This module picks the first
//! D3D11-aware synchronous H.264 decoder MFT and **checks that it really decodes on the GPU**:
//! every output sample must be a DXGI surface ([`DecodeError::NoHardwareDecoder`] otherwise,
//! e.g. a GPU without DXVA H.264 at that profile / size, where the MFT silently falls back to
//! software). See `adr/windows/0007-wmf-live-h264-decode.md`.
//!
//! **Latency.** `MF_LOW_LATENCY` (= `CODECAPI_AVLowLatencyMode`) is set, so a frame is output as
//! soon as it is decoded instead of after the DPB fills; for a stream without B-frames each
//! [`VideoDecoder::push_packet`] yields that packet's frame.
//!
//! **Loss.** After [`WmfLiveH264Decoder::reset`] (and at open) packets are dropped until one
//! with an IDR slice arrives, so the MFT never decodes against references it does not have.
//!
//! **Readback.** [`WmfLiveH264Decoder::read_nv12`] copies the last frame into caller buffers
//! through one reused staging texture: a GPU→CPU copy (costly, ~3 MB per 1080p frame), named
//! and opt-in, for consumers that cannot sample D3D11 textures.

#![allow(unsafe_code)]

use std::collections::VecDeque;

use crate::{DecodeError, VideoDecoder};
use mediaway_common::{
    Bytes, CodecKind, GpuBufferHandle, GpuDeviceHandle, NativeHandle, Packet, PixelFormat,
    Rational, StreamInfo, VideoFrame, VideoFrameStorage, VideoGeometry,
};
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11Texture2D};
use windows::Win32::Media::MediaFoundation::{
    IMFActivate, IMFDXGIDeviceManager, IMFSample, IMFTransform, MF_E_NOTACCEPTING, MF_LOW_LATENCY,
    MF_MT_FRAME_SIZE, MF_MT_MINIMUM_DISPLAY_APERTURE, MF_SA_D3D11_AWARE, MF_TRANSFORM_ASYNC,
    MFCreateDXGIDeviceManager, MFMediaType_Video, MFT_CATEGORY_VIDEO_DECODER, MFT_ENUM_FLAG,
    MFT_ENUM_FLAG_SORTANDFILTER, MFT_ENUM_FLAG_SYNCMFT, MFT_FRIENDLY_NAME_Attribute,
    MFT_MESSAGE_COMMAND_FLUSH, MFT_MESSAGE_SET_D3D_MANAGER, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES,
    MFT_REGISTER_TYPE_INFO, MFTEnumEx, MFVideoArea, MFVideoFormat_H264, MFVideoFormat_NV12,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::core::Interface;

use super::dx11;
use super::readback::Nv12Staging;
use super::runtime::{from_hns, unpack_u32_pair};
use super::shared::{
    Drain, begin_streaming, configure_decode_types, notify_end_streaming, payload_to_sample,
    process_one_output,
};

/// Configuration for [`WmfLiveH264Decoder::open`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WmfLiveH264Config {
    /// The D3D11 device to decode on ([`GpuDeviceHandle::DirectX11`]); output textures belong
    /// to it. The decoder takes a reference and turns on multithread protection
    /// (`ID3D11Multithread::SetMultithreadProtected`), since Media Foundation uses the device
    /// from its own calls.
    pub device: GpuDeviceHandle,
    /// Expected coded width (a hint for the first output type; the SPS decides, and a
    /// different or changing size is handled as a stream change).
    pub width: u32,
    /// Expected coded height (hint, see [`Self::width`]).
    pub height: u32,
    /// Timebase of packet / frame `pts` and `duration`.
    pub time_base: Rational,
}

impl WmfLiveH264Config {
    /// A config for `width` x `height` on `device` with microsecond timestamps.
    #[must_use]
    pub const fn new(device: GpuDeviceHandle, width: u32, height: u32) -> Self {
        Self {
            device,
            width,
            height,
            time_base: Rational::new(1, 1_000_000),
        }
    }
}

/// A decoded frame: the MFT's output sample (keeps the texture slice out of its pool) and the
/// texture behind it.
struct Hold {
    _sample: IMFSample,
    texture: ID3D11Texture2D,
    subresource: u32,
    pts: i64,
    duration: u64,
    width: u32,
    height: u32,
}

/// Live H.264 decoder: Annex-B access units in, D3D11 NV12 textures out (see module docs).
///
/// Frames from [`VideoDecoder::poll_frame`] are
/// [`VideoFrameStorage::Gpu`](mediaway_common::VideoFrameStorage::Gpu) with a
/// [`GpuBufferHandle::DirectX11`] texture **and subresource** (the MFT decodes into slices of
/// one texture array). The slice is the decoder's: it stays valid until the next
/// `push_packet`, `poll_frame`, [`Self::reset`] or `flush`. `width` / `height` are the shown
/// size (the SPS crop); the texture can be larger (1080p is coded as 1088 rows).
pub struct WmfLiveH264Decoder {
    transform: IMFTransform,
    _manager: IMFDXGIDeviceManager,
    device: ID3D11Device,
    staging: Nv12Staging,
    name: String,
    info: StreamInfo,
    time_base: Rational,
    pending: VecDeque<Hold>,
    /// The frame last returned by `poll_frame` (what [`Self::read_nv12`] reads).
    current: Option<Hold>,
    /// Shown size of the current output type.
    width: u32,
    height: u32,
    awaiting_idr: bool,
    dropped: u64,
    flushed: bool,
}

// SAFETY: Media Foundation transforms and D3D11 devices are free-threaded COM objects; the
// decoder is driven through `&mut self` only, so calls are never concurrent.
#[allow(
    clippy::non_send_fields_in_send_ty,
    reason = "COM objects owned by this session; see the SAFETY note"
)]
unsafe impl Send for WmfLiveH264Decoder {}

impl WmfLiveH264Decoder {
    /// Open the GPU H.264 decoder on `config.device`.
    ///
    /// # Errors
    ///
    /// - [`DecodeError::Unsupported`]: not a D3D11 device handle.
    /// - [`DecodeError::NoHardwareDecoder`]: no D3D11-aware H.264 decoder MFT accepts this
    ///   device (no DXVA), or the decoder would hand out system-memory frames. Fall back to a
    ///   software decoder.
    /// - [`DecodeError::Backend`]: a Media Foundation call failed.
    pub fn open(config: &WmfLiveH264Config) -> Result<Self, DecodeError> {
        let GpuDeviceHandle::DirectX11(handle) = config.device else {
            return Err(DecodeError::Unsupported);
        };
        if config.time_base.den == 0 {
            return Err(DecodeError::InvalidInput);
        }
        super::runtime::ensure_mf()?;
        let device = dx11::device_from_handle(handle)?;
        dx11::enable_multithread(&device);
        let manager = device_manager(&device)?;
        let (transform, name) = activate_d3d11_decoder(&manager)?;

        // SAFETY: attribute store of the transform we own.
        let attrs = unsafe { transform.GetAttributes() }.map_err(|_| DecodeError::Backend)?;
        // SAFETY: plain attribute setter.
        unsafe { attrs.SetUINT32(&MF_LOW_LATENCY, 1) }.map_err(|_| DecodeError::Backend)?;
        configure_decode_types(
            &transform,
            config.width,
            config.height,
            &Bytes::new(),
            &MFVideoFormat_H264,
        )?;
        // SAFETY: stream info query on a configured transform.
        let out_info =
            unsafe { transform.GetOutputStreamInfo(0) }.map_err(|_| DecodeError::Backend)?;
        if out_info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 == 0 {
            // A D3D11-mode decoder allocates its own DXGI output samples; one that wants
            // caller (system-memory) buffers is decoding in software.
            return Err(DecodeError::NoHardwareDecoder);
        }
        begin_streaming(&transform)?;
        let staging = Nv12Staging::new(&device)?;
        Ok(Self {
            transform,
            _manager: manager,
            device,
            staging,
            name,
            info: StreamInfo::Video {
                id: 0,
                codec: CodecKind::H264,
                time_base: config.time_base,
                geometry: VideoGeometry {
                    width: config.width,
                    height: config.height,
                },
                extra_data: Bytes::new(),
            },
            time_base: config.time_base,
            pending: VecDeque::with_capacity(4),
            current: None,
            width: config.width,
            height: config.height,
            awaiting_idr: true,
            dropped: 0,
            flushed: false,
        })
    }

    /// Friendly name of the decoder MFT in use (e.g. `"Microsoft H264 Video Decoder MFT"`),
    /// for logs.
    #[must_use]
    pub fn decoder_name(&self) -> &str {
        &self.name
    }

    /// Whether packets are being dropped until the next IDR (after open and [`Self::reset`]).
    /// While this is `true` the sender should be asked for a keyframe.
    #[must_use]
    pub const fn is_waiting_for_keyframe(&self) -> bool {
        self.awaiting_idr
    }

    /// Packets dropped so far while waiting for an IDR.
    #[must_use]
    pub const fn dropped_packets(&self) -> u64 {
        self.dropped
    }

    /// Recover from packet loss: discard every frame in flight (`MFT_MESSAGE_COMMAND_FLUSH`)
    /// and drop packets until the next IDR, so nothing is decoded against missing
    /// references. Call it when a gap in the stream is detected, then request a keyframe.
    ///
    /// # Errors
    ///
    /// [`DecodeError::Closed`] after `flush`; [`DecodeError::Backend`] if the MFT rejects the
    /// flush.
    pub fn reset(&mut self) -> Result<(), DecodeError> {
        if self.flushed {
            return Err(DecodeError::Closed);
        }
        self.current = None;
        self.pending.clear();
        // SAFETY: stream control message, no pointers.
        unsafe { self.transform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0) }
            .map_err(|_| DecodeError::Backend)?;
        self.awaiting_idr = true;
        Ok(())
    }

    /// Copy the frame last returned by `poll_frame` into caller-owned NV12 planes: `width`
    /// bytes of luma per row into `y` (rows `y_stride` apart), then `width` bytes of
    /// interleaved `CbCr` per row for `height / 2` rows into `uv` (rows `uv_stride` apart).
    /// Returns the frame's `(width, height)`.
    ///
    /// **Costly path (GPU→CPU readback):** a `CopySubresourceRegion` into a staging texture,
    /// then a blocking `Map` that waits for the GPU (decode + copy) to finish, then a row copy.
    /// The staging texture is created once and only re-created when the coded size changes;
    /// nothing is allocated per frame.
    ///
    /// # Errors
    ///
    /// [`DecodeError::InvalidInput`] when no frame is held (nothing polled since the last
    /// push / reset), or a buffer / stride is too small for the frame;
    /// [`DecodeError::Backend`] when a D3D11 call fails.
    pub fn read_nv12(
        &mut self,
        y: &mut [u8],
        y_stride: usize,
        uv: &mut [u8],
        uv_stride: usize,
    ) -> Result<(u32, u32), DecodeError> {
        let hold = self.current.as_ref().ok_or(DecodeError::InvalidInput)?;
        let (w, h) = (hold.width as usize, hold.height as usize);
        let uv_rows = h.div_ceil(2);
        let fits = |len: usize, stride: usize, rows: usize| {
            stride >= w && rows > 0 && len >= stride * (rows - 1) + w
        };
        if !fits(y.len(), y_stride, h) || !fits(uv.len(), uv_stride, uv_rows) {
            return Err(DecodeError::InvalidInput);
        }
        self.staging.read(
            &self.device,
            &hold.texture,
            hold.subresource,
            (hold.width, hold.height),
            (y, y_stride),
            (uv, uv_stride),
        )?;
        Ok((hold.width, hold.height))
    }

    fn decode(&mut self, packet: &Packet) -> Result<(), DecodeError> {
        let sample = payload_to_sample(
            &packet.payload,
            packet,
            self.time_base.num,
            self.time_base.den,
        )?;
        loop {
            // SAFETY: sample built above; HRESULT inspected.
            match unsafe { self.transform.ProcessInput(0, &sample, 0) } {
                Ok(()) => break,
                // Output must be taken first; then the same input is offered again.
                Err(e) if e.code() == MF_E_NOTACCEPTING => {
                    if !self.drain_output()? {
                        return Err(DecodeError::Backend);
                    }
                }
                Err(_) => return Err(DecodeError::Backend),
            }
        }
        self.drain_output()?;
        Ok(())
    }

    /// Take every frame the MFT has ready; whether any came out.
    fn drain_output(&mut self) -> Result<bool, DecodeError> {
        let mut any = false;
        loop {
            match process_one_output(&self.transform, true, 0)? {
                Drain::Sample(sample) => {
                    self.adopt(sample)?;
                    any = true;
                }
                Drain::NeedMore => return Ok(any),
                Drain::StreamChange => self.stream_change()?,
            }
        }
    }

    /// A new SPS changed the size: take the MFT's NV12 output type and the new shown size.
    fn stream_change(&mut self) -> Result<(), DecodeError> {
        super::video_cpu::negotiate_nv12_output_type(&self.transform)?;
        let (width, height) = shown_size(&self.transform)?;
        self.width = width;
        self.height = height;
        if let StreamInfo::Video { geometry, .. } = &mut self.info {
            *geometry = VideoGeometry { width, height };
        }
        Ok(())
    }

    fn adopt(&mut self, sample: IMFSample) -> Result<(), DecodeError> {
        // A system-memory sample means the MFT is decoding in software after all.
        let (texture, subresource) = dx11::texture_from_output_sample(&sample)
            .map_err(|_| DecodeError::NoHardwareDecoder)?;
        // SAFETY: plain getters on the MFT's output sample.
        let pts_hns = unsafe { sample.GetSampleTime() }.unwrap_or(0);
        // SAFETY: as above.
        let dur_hns = unsafe { sample.GetSampleDuration() }.unwrap_or(0);
        let pts = from_hns(pts_hns, self.time_base.num, self.time_base.den);
        let duration =
            u64::try_from(from_hns(dur_hns, self.time_base.num, self.time_base.den)).unwrap_or(0);
        self.pending.push_back(Hold {
            _sample: sample,
            texture,
            subresource,
            pts,
            duration,
            width: self.width,
            height: self.height,
        });
        Ok(())
    }
}

impl VideoDecoder for WmfLiveH264Decoder {
    fn stream_info(&self) -> &StreamInfo {
        &self.info
    }

    /// Decode one Annex-B access unit. While [`WmfLiveH264Decoder::is_waiting_for_keyframe`],
    /// packets without an IDR slice are dropped (`Ok`, counted in
    /// [`WmfLiveH264Decoder::dropped_packets`]).
    fn push_packet(&mut self, packet: &Packet) -> Result<(), DecodeError> {
        if self.flushed {
            return Err(DecodeError::Closed);
        }
        self.current = None;
        if packet.is_discard {
            return Ok(());
        }
        if packet.payload.is_empty() {
            return Err(DecodeError::InvalidInput);
        }
        if self.awaiting_idr {
            if !has_idr_slice(&packet.payload) {
                self.dropped += 1;
                return Ok(());
            }
            self.awaiting_idr = false;
        }
        self.decode(packet)
    }

    fn poll_frame(&mut self) -> Result<Option<VideoFrame>, DecodeError> {
        self.current = None;
        let Some(hold) = self.pending.pop_front() else {
            return Ok(None);
        };
        let texture =
            NativeHandle::new(hold.texture.as_raw() as usize).ok_or(DecodeError::Backend)?;
        let frame = VideoFrame {
            pts: hold.pts,
            duration: hold.duration,
            width: hold.width,
            height: hold.height,
            format: PixelFormat::Nv12,
            storage: VideoFrameStorage::Gpu(GpuBufferHandle::DirectX11 {
                texture,
                subresource: hold.subresource,
            }),
        };
        self.current = Some(hold);
        Ok(Some(frame))
    }

    /// Drain the frames still inside the MFT (there are none for a B-frame-free stream) and
    /// end the session.
    fn flush(&mut self) -> Result<(), DecodeError> {
        if self.flushed {
            return Ok(());
        }
        self.flushed = true;
        self.current = None;
        // SAFETY: stream control messages, no pointers.
        unsafe {
            self.transform
                .ProcessMessage(
                    windows::Win32::Media::MediaFoundation::MFT_MESSAGE_NOTIFY_END_OF_STREAM,
                    0,
                )
                .map_err(|_| DecodeError::Backend)?;
            self.transform
                .ProcessMessage(
                    windows::Win32::Media::MediaFoundation::MFT_MESSAGE_COMMAND_DRAIN,
                    0,
                )
                .map_err(|_| DecodeError::Backend)?;
        }
        self.drain_output()?;
        notify_end_streaming(&self.transform);
        Ok(())
    }
}

/// Whether Annex-B `data` has an IDR slice (NAL type 5). Scans start codes only up to the
/// first slice NAL, so the cost is the few header bytes in front of the slice data.
const fn has_idr_slice(data: &[u8]) -> bool {
    let mut i = 0;
    while i + 3 < data.len() {
        if data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 {
            match data[i + 3] & 0x1F {
                5 => return true,
                1..=4 => return false,
                _ => {}
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    false
}

fn device_manager(device: &ID3D11Device) -> Result<IMFDXGIDeviceManager, DecodeError> {
    let mut token = 0u32;
    let mut manager: Option<IMFDXGIDeviceManager> = None;
    // SAFETY: out-params for the token and the manager.
    unsafe { MFCreateDXGIDeviceManager(&raw mut token, &raw mut manager) }
        .map_err(|_| DecodeError::Backend)?;
    let manager = manager.ok_or(DecodeError::Backend)?;
    // SAFETY: associates our live device with the manager.
    unsafe { manager.ResetDevice(device, token) }.map_err(|_| DecodeError::Backend)?;
    Ok(manager)
}

/// The first synchronous, D3D11-aware H.264 decoder MFT that accepts `manager`, and its name.
fn activate_d3d11_decoder(
    manager: &IMFDXGIDeviceManager,
) -> Result<(IMFTransform, String), DecodeError> {
    let input = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_H264,
    };
    let output = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_NV12,
    };
    let flags = MFT_ENUM_FLAG(MFT_ENUM_FLAG_SYNCMFT.0 | MFT_ENUM_FLAG_SORTANDFILTER.0);
    let mut activates: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count = 0u32;
    // SAFETY: MFTEnumEx writes a CoTaskMem array of activates and its length.
    unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_DECODER,
            flags,
            Some(&raw const input),
            Some(&raw const output),
            &raw mut activates,
            &raw mut count,
        )
    }
    .map_err(|_| DecodeError::Backend)?;
    if activates.is_null() {
        return Err(DecodeError::NoHardwareDecoder);
    }
    let mut chosen = None;
    for i in 0..count as usize {
        // SAFETY: `activates` holds `count` slots; each is taken (moved out) exactly once.
        let Some(activate) = (unsafe { (*activates.add(i)).take() }) else {
            continue;
        };
        if chosen.is_none() {
            chosen = try_activate(&activate, manager);
        }
    }
    // SAFETY: the array was CoTaskMemAlloc'd by MFTEnumEx; every slot was taken above.
    unsafe { CoTaskMemFree(Some(activates.cast_const().cast())) };
    chosen.ok_or(DecodeError::NoHardwareDecoder)
}

fn try_activate(
    activate: &IMFActivate,
    manager: &IMFDXGIDeviceManager,
) -> Option<(IMFTransform, String)> {
    // SAFETY: activates the MFT the enumeration returned.
    let transform: IMFTransform = unsafe { activate.ActivateObject() }.ok()?;
    // SAFETY: attribute getters on the new transform.
    let attrs = unsafe { transform.GetAttributes() }.ok()?;
    // SAFETY: as above.
    let d3d11_aware = unsafe { attrs.GetUINT32(&MF_SA_D3D11_AWARE) }.unwrap_or(0) != 0;
    // SAFETY: as above.
    let is_async = unsafe { attrs.GetUINT32(&MF_TRANSFORM_ASYNC) }.unwrap_or(0) != 0;
    if !d3d11_aware || is_async {
        return None;
    }
    // SAFETY: SET_D3D_MANAGER takes the manager's raw `IUnknown*`, AddRef'd by the MFT.
    unsafe { transform.ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, manager.as_raw() as usize) }
        .ok()?;
    Some((transform, friendly_name(activate)))
}

fn friendly_name(activate: &IMFActivate) -> String {
    let mut ptr = windows::core::PWSTR::null();
    let mut len = 0u32;
    // SAFETY: GetAllocatedString returns a CoTaskMem string we free below.
    if unsafe {
        activate.GetAllocatedString(&MFT_FRIENDLY_NAME_Attribute, &raw mut ptr, &raw mut len)
    }
    .is_err()
        || ptr.is_null()
    {
        return String::new();
    }
    // SAFETY: `ptr` is a valid NUL-terminated wide string from the call above.
    let name = unsafe { ptr.to_string() }.unwrap_or_default();
    // SAFETY: allocated by GetAllocatedString.
    unsafe { CoTaskMemFree(Some(ptr.0.cast_const().cast())) };
    name
}

/// The shown size of the current output type: the minimum display aperture when the MFT
/// sets one (the SPS crop, e.g. 1920 x 1080 of a 1920 x 1088 coded picture), else the frame
/// size.
fn shown_size(transform: &IMFTransform) -> Result<(u32, u32), DecodeError> {
    // SAFETY: current output type of a configured transform.
    let mt = unsafe { transform.GetOutputCurrentType(0) }.map_err(|_| DecodeError::Backend)?;
    let mut area = MFVideoArea::default();
    let size = std::mem::size_of::<MFVideoArea>();
    // SAFETY: `area` is a plain struct of exactly `size` bytes.
    let aperture = unsafe {
        mt.GetBlob(
            &MF_MT_MINIMUM_DISPLAY_APERTURE,
            std::slice::from_raw_parts_mut((&raw mut area).cast::<u8>(), size),
            None,
        )
    };
    if aperture.is_ok() && area.Area.cx > 0 && area.Area.cy > 0 {
        return Ok((area.Area.cx.unsigned_abs(), area.Area.cy.unsigned_abs()));
    }
    // SAFETY: as above.
    let packed = unsafe { mt.GetUINT64(&MF_MT_FRAME_SIZE) }.map_err(|_| DecodeError::Backend)?;
    Ok(unpack_u32_pair(packed))
}

#[cfg(test)]
#[path = "live_tests.rs"]
mod tests;
