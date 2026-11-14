//! Windows NVENC live H.264 session over the hand-written [`super::ffi`] subset.
//!
//! Synchronous mode (`enableEncodeAsync = 0`, one output bitstream buffer): `encode_d3d11`
//! maps the input, encodes, then `nvEncLockBitstream` waits for the hardware, so the packet is
//! complete when it returns. With no B-frames and zero reorder delay NVENC never returns
//! `NV_ENC_ERR_NEED_MORE_INPUT`, so one input always gives one output.

use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::OnceLock;

use mediaway_common::{
    Bytes, CodecKind, GpuDeviceHandle, NativeHandle, Packet, StreamInfo, VideoGeometry,
};
use windows::Win32::Graphics::Direct3D11::{D3D11_TEXTURE2D_DESC, ID3D11Device, ID3D11Texture2D};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_B8G8R8A8_UNORM_SRGB,
    DXGI_FORMAT_B8G8R8X8_UNORM, DXGI_FORMAT_NV12, DXGI_FORMAT_R8G8B8A8_UNORM,
    DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
};
use windows::core::Interface;

use super::ffi::{self, Config, FunctionList, InitializeParams, zeroed};
use super::{NvencLatency, NvencLiveConfig, NvencLiveInput};
use crate::EncodeError;

/// Registered textures kept at once (see `NvencLiveEncoder::encode_d3d11`).
const MAX_REGISTRATIONS: usize = 8;

/// `NvEncodeAPICreateInstance`, resolved once per process. The DLL stays loaded for the
/// process lifetime (never `FreeLibrary`d), so the pointer stays valid.
static CREATE_INSTANCE: OnceLock<Option<ffi::FnCreateInstance>> = OnceLock::new();

fn create_instance() -> Result<ffi::FnCreateInstance, EncodeError> {
    CREATE_INSTANCE
        .get_or_init(load_nvenc)
        .ok_or(EncodeError::Backend)
}

/// Load `nvEncodeAPI64.dll` from System32 (the driver installs it there) and check that it
/// supports API 13.0.
fn load_nvenc() -> Option<ffi::FnCreateInstance> {
    use windows::Win32::System::LibraryLoader::{
        GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
    };
    // SAFETY: loads a system DLL by name; the handle is intentionally leaked (process-lifetime
    // load, see `CREATE_INSTANCE`).
    let module = unsafe {
        LoadLibraryExW(
            windows::core::w!("nvEncodeAPI64.dll"),
            None,
            LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
    }
    .ok()?;
    // SAFETY: `module` is the live module loaded above; names are NUL-terminated literals.
    let create = unsafe { GetProcAddress(module, windows::core::s!("NvEncodeAPICreateInstance")) }?;
    // SAFETY: as above.
    let max_version = unsafe {
        GetProcAddress(
            module,
            windows::core::s!("NvEncodeAPIGetMaxSupportedVersion"),
        )
    }?;
    // SAFETY: both exports have exactly these C signatures in every NVENC driver
    // (`NVENCSTATUS NVENCAPI f(NV_ENCODE_API_FUNCTION_LIST*)` / `f(uint32_t*)`); `FARPROC`
    // and the target are both plain function pointers of the same size.
    let (create, max_version) = unsafe {
        (
            std::mem::transmute::<unsafe extern "system" fn() -> isize, ffi::FnCreateInstance>(
                create,
            ),
            std::mem::transmute::<
                unsafe extern "system" fn() -> isize,
                ffi::FnGetMaxSupportedVersion,
            >(max_version),
        )
    };
    let mut version = 0u32;
    // SAFETY: `version` is a valid out-param.
    if unsafe { max_version(&raw mut version) } != ffi::NV_ENC_SUCCESS {
        return None;
    }
    let wanted = (ffi::NVENCAPI_MAJOR_VERSION << 4) | ffi::NVENCAPI_MINOR_VERSION;
    (version >= wanted).then_some(create)
}

const fn check(status: ffi::Status) -> Result<(), EncodeError> {
    if status == ffi::NV_ENC_SUCCESS {
        Ok(())
    } else {
        Err(EncodeError::Backend)
    }
}

/// One texture registered with NVENC. Holds a COM reference so the address stays unique.
struct Registration {
    texture: ID3D11Texture2D,
    registered: *mut c_void,
}

pub(super) struct LiveSession {
    api: Box<FunctionList>,
    encoder: *mut c_void,
    bitstream: *mut c_void,
    /// Pointed to by `init.encode_config` only for the duration of an NVENC call.
    config: Box<Config>,
    /// `encode_config` is kept null here and patched in at call time.
    init: InitializeParams,
    registrations: Vec<Registration>,
    pending: VecDeque<Packet>,
    buffer_format: u32,
    input: NvencLiveInput,
    width: u32,
    height: u32,
    max_width: u32,
    max_height: u32,
    bitrate: u32,
    vbv: u32,
    dyn_bitrate: bool,
    force_idr: bool,
    frame_idx: u32,
    flushed: bool,
    info: StreamInfo,
    /// Dropped after `Drop::drop` has destroyed the NVENC session that referenced it.
    _device: ID3D11Device,
}

// SAFETY: the NVENC session handle and the D3D11 device / textures it references have no
// thread affinity (D3D11 devices are free-threaded; NVENC requires only that calls on one
// session are not concurrent, which `&mut self` guarantees). The raw pointers are owned by
// this session and never shared.
#[allow(
    clippy::non_send_fields_in_send_ty,
    reason = "the raw NVENC pointers are owned by this session; see the SAFETY note"
)]
unsafe impl Send for LiveSession {}

impl LiveSession {
    pub(super) fn open(cfg: &NvencLiveConfig) -> Result<Self, EncodeError> {
        let (max_width, max_height) = validate(cfg)?;
        let GpuDeviceHandle::DirectX11(device_handle) = cfg.device else {
            return Err(EncodeError::Unsupported);
        };
        let device = device_from_handle(device_handle)?;
        let create = create_instance()?;

        let mut api: Box<FunctionList> = Box::new(zeroed());
        api.version = ffi::NV_ENCODE_API_FUNCTION_LIST_VER;
        // SAFETY: `api` is a zeroed, versioned function list the driver fills in.
        check(unsafe { create(&raw mut *api) })?;
        let open = api.open_encode_session_ex.ok_or(EncodeError::Backend)?;

        let mut params: ffi::OpenEncodeSessionExParams = zeroed();
        params.version = ffi::NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS_VER;
        params.device_type = ffi::NV_ENC_DEVICE_TYPE_DIRECTX;
        params.device = device.as_raw();
        params.api_version = ffi::NVENCAPI_VERSION;
        let mut encoder = std::ptr::null_mut();
        // SAFETY: `params` is versioned and names a live D3D11 device; `encoder` is an
        // out-param.
        check(unsafe { open(&raw mut params, &raw mut encoder) })?;
        if encoder.is_null() {
            return Err(EncodeError::Backend);
        }

        let buffer_format = match cfg.input {
            NvencLiveInput::Bgra => ffi::NV_ENC_BUFFER_FORMAT_ARGB,
            NvencLiveInput::Rgba => ffi::NV_ENC_BUFFER_FORMAT_ABGR,
            NvencLiveInput::Nv12 => ffi::NV_ENC_BUFFER_FORMAT_NV12,
        };
        let vbv = vbv_bits(
            cfg.bitrate_bps,
            cfg.vbv_buffer_bits,
            cfg.fps_num,
            cfg.fps_den,
        );
        // From here on `session`'s `Drop` destroys the encoder on any error.
        let mut session = Self {
            api,
            encoder,
            bitstream: std::ptr::null_mut(),
            config: Box::new(zeroed_config()),
            init: zeroed(),
            registrations: Vec::with_capacity(MAX_REGISTRATIONS),
            pending: VecDeque::with_capacity(2),
            buffer_format,
            input: cfg.input,
            width: cfg.width,
            height: cfg.height,
            max_width,
            max_height,
            bitrate: cfg.bitrate_bps,
            vbv,
            dyn_bitrate: false,
            force_idr: true,
            frame_idx: 0,
            flushed: false,
            info: stream_info(cfg.width, cfg.height, cfg),
            _device: device,
        };
        session.initialize(cfg)?;
        Ok(session)
    }

    fn initialize(&mut self, cfg: &NvencLiveConfig) -> Result<(), EncodeError> {
        if !self.supports_h264()? {
            return Err(EncodeError::Unsupported);
        }
        if self.cap(ffi::NV_ENC_CAPS_WIDTH_MAX)? < i64::from(self.max_width)
            || self.cap(ffi::NV_ENC_CAPS_HEIGHT_MAX)? < i64::from(self.max_height)
        {
            return Err(EncodeError::Unsupported);
        }
        if cfg.intra_refresh.is_some() && self.cap(ffi::NV_ENC_CAPS_SUPPORT_INTRA_REFRESH)? == 0 {
            return Err(EncodeError::Unsupported);
        }
        self.dyn_bitrate = self.cap(ffi::NV_ENC_CAPS_SUPPORT_DYN_BITRATE_CHANGE)? != 0;

        let preset_guid = ffi::NV_ENC_PRESET_GUIDS[cfg.preset.index()];
        let tuning = match cfg.latency {
            NvencLatency::UltraLow => ffi::NV_ENC_TUNING_INFO_ULTRA_LOW_LATENCY,
            NvencLatency::Low => ffi::NV_ENC_TUNING_INFO_LOW_LATENCY,
        };
        let get_preset = self
            .api
            .get_encode_preset_config_ex
            .ok_or(EncodeError::Backend)?;
        let mut preset: Box<ffi::PresetConfig> = Box::new(zeroed());
        preset.version = ffi::NV_ENC_PRESET_CONFIG_VER;
        preset.preset_cfg.version = ffi::NV_ENC_CONFIG_VER;
        // SAFETY: live encoder; `preset` is a zeroed, versioned out-struct.
        check(unsafe {
            get_preset(
                self.encoder,
                ffi::NV_ENC_CODEC_H264_GUID,
                preset_guid,
                tuning,
                &raw mut *preset,
            )
        })?;
        *self.config = preset.preset_cfg;
        apply_live_config(&mut self.config, self.bitrate, self.vbv, cfg);

        let init = &mut self.init;
        init.version = ffi::NV_ENC_INITIALIZE_PARAMS_VER;
        init.encode_guid = ffi::NV_ENC_CODEC_H264_GUID;
        init.preset_guid = preset_guid;
        init.encode_width = self.width;
        init.encode_height = self.height;
        init.dar_width = self.width;
        init.dar_height = self.height;
        init.frame_rate_num = cfg.fps_num;
        init.frame_rate_den = cfg.fps_den;
        init.enable_encode_async = 0;
        init.enable_ptd = 1;
        init.max_encode_width = self.max_width;
        init.max_encode_height = self.max_height;
        init.tuning_info = tuning;
        init.buffer_format = self.buffer_format;

        let initialize = self.api.initialize_encoder.ok_or(EncodeError::Backend)?;
        let mut init = self.init;
        init.encode_config = &raw mut *self.config;
        // SAFETY: `init` is versioned and points at `self.config`, alive for the call.
        check(unsafe { initialize(self.encoder, &raw mut init) })?;

        let create_bitstream = self
            .api
            .create_bitstream_buffer
            .ok_or(EncodeError::Backend)?;
        let mut buf: ffi::CreateBitstreamBuffer = zeroed();
        buf.version = ffi::NV_ENC_CREATE_BITSTREAM_BUFFER_VER;
        // SAFETY: initialized encoder; `buf` is a versioned out-struct.
        check(unsafe { create_bitstream(self.encoder, &raw mut buf) })?;
        if buf.bitstream_buffer.is_null() {
            return Err(EncodeError::Backend);
        }
        self.bitstream = buf.bitstream_buffer;
        Ok(())
    }

    fn supports_h264(&self) -> Result<bool, EncodeError> {
        let count_fn = self.api.get_encode_guid_count.ok_or(EncodeError::Backend)?;
        let list_fn = self.api.get_encode_guids.ok_or(EncodeError::Backend)?;
        let mut count = 0u32;
        // SAFETY: open encoder; `count` is an out-param.
        check(unsafe { count_fn(self.encoder, &raw mut count) })?;
        let mut guids = vec![ffi::Guid::default(); count as usize];
        let mut written = 0u32;
        // SAFETY: `guids` holds `count` elements, the size passed in.
        check(unsafe { list_fn(self.encoder, guids.as_mut_ptr(), count, &raw mut written) })?;
        guids.truncate(written as usize);
        Ok(guids.contains(&ffi::NV_ENC_CODEC_H264_GUID))
    }

    fn cap(&self, caps: u32) -> Result<i64, EncodeError> {
        let get_caps = self.api.get_encode_caps.ok_or(EncodeError::Backend)?;
        let mut param: ffi::CapsParam = zeroed();
        param.version = ffi::NV_ENC_CAPS_PARAM_VER;
        param.caps_to_query = caps;
        let mut value = 0i32;
        // SAFETY: open encoder; `param` is versioned; `value` is an out-param.
        check(unsafe {
            get_caps(
                self.encoder,
                ffi::NV_ENC_CODEC_H264_GUID,
                &raw mut param,
                &raw mut value,
            )
        })?;
        Ok(i64::from(value))
    }

    pub(super) const fn request_keyframe(&mut self) {
        self.force_idr = true;
    }

    pub(super) const fn bitrate(&self) -> u32 {
        self.bitrate
    }

    pub(super) const fn stream_info(&self) -> &StreamInfo {
        &self.info
    }

    pub(super) fn poll_packet(&mut self) -> Option<Packet> {
        self.pending.pop_front()
    }

    pub(super) fn set_bitrate(&mut self, bitrate_bps: u32) -> Result<(), EncodeError> {
        if bitrate_bps == 0 {
            return Err(EncodeError::InvalidInput);
        }
        if !self.dyn_bitrate {
            return Err(EncodeError::Unsupported);
        }
        // Keep the VBV the same number of frames long.
        let vbv = u64::from(self.vbv) * u64::from(bitrate_bps) / u64::from(self.bitrate.max(1));
        let vbv = u32::try_from(vbv).unwrap_or(u32::MAX).max(1);
        let mut config = *self.config;
        set_rate(&mut config, bitrate_bps, vbv);
        self.reconfigure(config, self.init, 0)?;
        self.bitrate = bitrate_bps;
        self.vbv = vbv;
        Ok(())
    }

    pub(super) fn resize(&mut self, width: u32, height: u32) -> Result<(), EncodeError> {
        if width == 0
            || height == 0
            || !width.is_multiple_of(2)
            || !height.is_multiple_of(2)
            || width > self.max_width
            || height > self.max_height
        {
            return Err(EncodeError::InvalidInput);
        }
        let mut init = self.init;
        init.encode_width = width;
        init.encode_height = height;
        init.dar_width = width;
        init.dar_height = height;
        self.reconfigure(
            *self.config,
            init,
            ffi::RECONFIGURE_RESET_ENCODER | ffi::RECONFIGURE_FORCE_IDR,
        )?;
        self.release_registrations();
        self.width = width;
        self.height = height;
        if let StreamInfo::Video { geometry, .. } = &mut self.info {
            *geometry = VideoGeometry { width, height };
        }
        self.force_idr = true;
        Ok(())
    }

    /// `nvEncReconfigureEncoder` with `config` / `init`; commits them on success.
    fn reconfigure(
        &mut self,
        mut config: Config,
        mut init: InitializeParams,
        flags: u32,
    ) -> Result<(), EncodeError> {
        if self.flushed {
            return Err(EncodeError::Closed);
        }
        let reconfigure = self.api.reconfigure_encoder.ok_or(EncodeError::Backend)?;
        init.encode_config = &raw mut config;
        let mut params: ffi::ReconfigureParams = zeroed();
        params.version = ffi::NV_ENC_RECONFIGURE_PARAMS_VER;
        params.re_init_encode_params = init;
        params.flags = flags;
        // SAFETY: initialized encoder; `params` is versioned and its init params point at
        // `config`, alive for the call.
        check(unsafe { reconfigure(self.encoder, &raw mut params) })?;
        init.encode_config = std::ptr::null_mut();
        *self.config = config;
        self.init = init;
        Ok(())
    }

    pub(super) fn encode_d3d11(
        &mut self,
        texture: NativeHandle,
        subresource: u32,
        pts: i64,
        duration: u64,
    ) -> Result<(), EncodeError> {
        if self.flushed {
            return Err(EncodeError::Closed);
        }
        if subresource != 0 {
            return Err(EncodeError::Unsupported);
        }
        let registered = self.registration(texture)?;

        let map_fn = self.api.map_input_resource.ok_or(EncodeError::Backend)?;
        let mut map: ffi::MapInputResource = zeroed();
        map.version = ffi::NV_ENC_MAP_INPUT_RESOURCE_VER;
        map.registered_resource = registered;
        // SAFETY: `registered` is a live registration of this encoder.
        check(unsafe { map_fn(self.encoder, &raw mut map) })?;
        let encoded = self.encode_mapped(map.mapped_resource, map.mapped_buffer_fmt, pts, duration);
        let unmap = self.api.unmap_input_resource.ok_or(EncodeError::Backend)?;
        // SAFETY: `map.mapped_resource` was mapped just above and the encode that read it has
        // completed (the bitstream lock in `encode_mapped` waited for it).
        let unmapped = check(unsafe { unmap(self.encoder, map.mapped_resource) });
        encoded?;
        unmapped
    }

    fn encode_mapped(
        &mut self,
        input: *mut c_void,
        buffer_fmt: u32,
        pts: i64,
        duration: u64,
    ) -> Result<(), EncodeError> {
        let encode = self.api.encode_picture.ok_or(EncodeError::Backend)?;
        let lock_fn = self.api.lock_bitstream.ok_or(EncodeError::Backend)?;
        let unlock = self.api.unlock_bitstream.ok_or(EncodeError::Backend)?;

        let mut pic: ffi::PicParams = zeroed();
        pic.version = ffi::NV_ENC_PIC_PARAMS_VER;
        pic.input_width = self.width;
        pic.input_height = self.height;
        pic.input_pitch = self.width;
        pic.encode_pic_flags = if self.force_idr {
            ffi::NV_ENC_PIC_FLAG_FORCEIDR | ffi::NV_ENC_PIC_FLAG_OUTPUT_SPSPPS
        } else {
            0
        };
        pic.frame_idx = self.frame_idx;
        pic.input_time_stamp = u64::from(self.frame_idx);
        pic.input_buffer = input;
        pic.output_bitstream = self.bitstream;
        pic.buffer_fmt = buffer_fmt;
        pic.picture_struct = ffi::NV_ENC_PIC_STRUCT_FRAME;
        // SAFETY: `pic` is versioned; `input` is a mapped input of this encoder and
        // `self.bitstream` its output buffer.
        let status = unsafe { encode(self.encoder, &raw mut pic) };
        if status == ffi::NV_ENC_ERR_NEED_MORE_INPUT {
            // Not expected without B-frames / lookahead; nothing to collect yet.
            self.frame_idx = self.frame_idx.wrapping_add(1);
            return Ok(());
        }
        check(status)?;
        self.frame_idx = self.frame_idx.wrapping_add(1);
        self.force_idr = false;

        let mut lock: ffi::LockBitstream = zeroed();
        lock.version = ffi::NV_ENC_LOCK_BITSTREAM_VER;
        lock.output_bitstream = self.bitstream;
        // SAFETY: `lock` is versioned and names this encoder's bitstream buffer; flags = 0
        // waits for the encode to finish.
        check(unsafe { lock_fn(self.encoder, &raw mut lock) })?;
        let len = lock.bitstream_size_in_bytes as usize;
        let payload = if lock.bitstream_buffer_ptr.is_null() || len == 0 {
            Bytes::new()
        } else {
            // SAFETY: while locked, `bitstream_buffer_ptr` points at `len` valid bytes. The
            // copy is required: the buffer is reused by the next frame, a `Packet` owns its
            // payload.
            Bytes::copy_from_slice(unsafe {
                std::slice::from_raw_parts(lock.bitstream_buffer_ptr.cast::<u8>(), len)
            })
        };
        let is_keyframe = lock.picture_type == ffi::NV_ENC_PIC_TYPE_IDR;
        // SAFETY: the buffer was locked just above.
        check(unsafe { unlock(self.encoder, self.bitstream) })?;
        self.pending.push_back(Packet {
            stream_id: 0,
            pts,
            dts: pts,
            duration,
            is_keyframe,
            is_discard: false,
            payload,
        });
        Ok(())
    }

    /// The NVENC registration for `texture`, registering (and caching) it on first use.
    fn registration(&mut self, texture: NativeHandle) -> Result<*mut c_void, EncodeError> {
        let raw = texture.get() as *mut c_void;
        if let Some(r) = self
            .registrations
            .iter()
            .find(|r| r.texture.as_raw() == raw)
        {
            return Ok(r.registered);
        }
        // SAFETY: the caller passes a live `ID3D11Texture2D*` (see `encode_d3d11` docs).
        let borrowed =
            unsafe { ID3D11Texture2D::from_raw_borrowed(&raw) }.ok_or(EncodeError::InvalidInput)?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: `borrowed` is a live texture; `desc` is an out-param.
        unsafe { borrowed.GetDesc(&raw mut desc) };
        if desc.Width < self.width
            || desc.Height < self.height
            || !format_matches(self.input, desc.Format)
        {
            return Err(EncodeError::InvalidInput);
        }
        if self.registrations.len() == MAX_REGISTRATIONS {
            let oldest = self.registrations.remove(0);
            self.unregister(&oldest);
        }
        let register = self.api.register_resource.ok_or(EncodeError::Backend)?;
        let mut reg: ffi::RegisterResource = zeroed();
        reg.version = ffi::NV_ENC_REGISTER_RESOURCE_VER;
        reg.resource_type = ffi::NV_ENC_INPUT_RESOURCE_TYPE_DIRECTX;
        reg.width = self.width;
        reg.height = self.height;
        reg.pitch = 0;
        reg.resource_to_register = raw;
        reg.buffer_format = self.buffer_format;
        reg.buffer_usage = ffi::NV_ENC_INPUT_IMAGE;
        // SAFETY: versioned struct naming a live texture on the session's device.
        check(unsafe { register(self.encoder, &raw mut reg) })?;
        if reg.registered_resource.is_null() {
            return Err(EncodeError::Backend);
        }
        self.registrations.push(Registration {
            texture: borrowed.clone(), // clone: COM AddRef so the cached address stays this texture
            registered: reg.registered_resource,
        });
        Ok(reg.registered_resource)
    }

    fn unregister(&self, registration: &Registration) {
        if let Some(unregister) = self.api.unregister_resource {
            // SAFETY: `registration` was registered with this encoder and is not mapped (maps
            // never outlive `encode_d3d11`). A failure leaves nothing to clean up on our side.
            let _ = unsafe { unregister(self.encoder, registration.registered) };
        }
    }

    fn release_registrations(&mut self) {
        while let Some(r) = self.registrations.pop() {
            self.unregister(&r);
        }
    }

    pub(super) fn flush(&mut self) -> Result<(), EncodeError> {
        if self.flushed {
            return Ok(());
        }
        self.flushed = true;
        let encode = self.api.encode_picture.ok_or(EncodeError::Backend)?;
        let mut pic: ffi::PicParams = zeroed();
        pic.version = ffi::NV_ENC_PIC_PARAMS_VER;
        pic.encode_pic_flags = ffi::NV_ENC_PIC_FLAG_EOS;
        // SAFETY: versioned end-of-stream picture with no input / output.
        check(unsafe { encode(self.encoder, &raw mut pic) })
    }
}

impl Drop for LiveSession {
    fn drop(&mut self) {
        self.release_registrations();
        if !self.bitstream.is_null()
            && let Some(destroy) = self.api.destroy_bitstream_buffer
        {
            // SAFETY: created by this encoder, not locked (locks never outlive a call).
            let _ = unsafe { destroy(self.encoder, self.bitstream) };
        }
        if let Some(destroy) = self.api.destroy_encoder {
            // SAFETY: `encoder` is the live session opened in `open`; nothing uses it after.
            let _ = unsafe { destroy(self.encoder) };
        }
    }
}

fn validate(cfg: &NvencLiveConfig) -> Result<(u32, u32), EncodeError> {
    let max_width = if cfg.max_width == 0 {
        cfg.width
    } else {
        cfg.max_width
    };
    let max_height = if cfg.max_height == 0 {
        cfg.height
    } else {
        cfg.max_height
    };
    let bad_size = cfg.width == 0
        || cfg.height == 0
        || !cfg.width.is_multiple_of(2)
        || !cfg.height.is_multiple_of(2)
        || max_width < cfg.width
        || max_height < cfg.height;
    let bad_rate = cfg.fps_num == 0 || cfg.fps_den == 0 || cfg.bitrate_bps == 0;
    let bad_refresh = cfg.intra_refresh.is_some_and(|ir| {
        ir.period_frames == 0 || ir.duration_frames == 0 || ir.duration_frames > ir.period_frames
    });
    if bad_size || bad_rate || bad_refresh || cfg.time_base.den == 0 {
        return Err(EncodeError::InvalidInput);
    }
    Ok((max_width, max_height))
}

/// `explicit`, or one frame's worth of bits at `bitrate`.
fn vbv_bits(bitrate: u32, explicit: u32, fps_num: u32, fps_den: u32) -> u32 {
    if explicit != 0 {
        return explicit;
    }
    let one_frame = u64::from(bitrate) * u64::from(fps_den) / u64::from(fps_num.max(1));
    u32::try_from(one_frame).unwrap_or(u32::MAX).max(1)
}

const fn set_rate(config: &mut Config, bitrate: u32, vbv: u32) {
    let rc = &mut config.rc_params;
    rc.rate_control_mode = ffi::NV_ENC_PARAMS_RC_CBR;
    rc.average_bit_rate = bitrate;
    rc.max_bit_rate = bitrate;
    rc.vbv_buffer_size = vbv;
    rc.vbv_initial_delay = vbv;
}

/// Turn the driver's preset config into the live configuration described in the module docs.
const fn apply_live_config(config: &mut Config, bitrate: u32, vbv: u32, cfg: &NvencLiveConfig) {
    config.profile_guid = ffi::NV_ENC_H264_PROFILE_HIGH_GUID;
    config.gop_length = ffi::NVENC_INFINITE_GOPLENGTH;
    config.frame_interval_p = 1;
    set_rate(config, bitrate, vbv);
    config.rc_params.flags &= !ffi::RC_ENABLE_LOOKAHEAD;
    config.rc_params.flags |= ffi::RC_ZERO_REORDER_DELAY;
    config.rc_params.lookahead_depth = 0;
    // SAFETY: the session encodes H.264, so the driver filled the union's H.264 arm and that
    // is the arm read and written here (all fields are plain integers / pointers).
    let h264 = unsafe { &mut config.encode_codec_config.h264 };
    h264.idr_period = ffi::NVENC_INFINITE_GOPLENGTH;
    h264.flags |= ffi::H264_REPEAT_SPSPPS;
    if let Some(ir) = cfg.intra_refresh {
        h264.flags |= ffi::H264_ENABLE_INTRA_REFRESH | ffi::H264_OUTPUT_RECOVERY_POINT_SEI;
        h264.intra_refresh_period = ir.period_frames;
        h264.intra_refresh_cnt = ir.duration_frames;
    }
    // Colour signalling for NVENC's RGB -> YUV conversion: BT.709, limited range.
    let vui = &mut h264.vui;
    vui.video_signal_type_present_flag = 1;
    vui.video_format = ffi::NV_ENC_VUI_VIDEO_FORMAT_UNSPECIFIED;
    vui.video_full_range_flag = 0;
    vui.colour_description_present_flag = 1;
    vui.colour_primaries = ffi::NV_ENC_VUI_BT709;
    vui.transfer_characteristics = ffi::NV_ENC_VUI_BT709;
    vui.colour_matrix = ffi::NV_ENC_VUI_BT709;
}

const fn zeroed_config() -> Config {
    // SAFETY: `Config` is plain integers / pointers / a union of those: all-zero is valid.
    unsafe { std::mem::zeroed() }
}

fn format_matches(input: NvencLiveInput, format: DXGI_FORMAT) -> bool {
    match input {
        NvencLiveInput::Bgra => [
            DXGI_FORMAT_B8G8R8A8_UNORM,
            DXGI_FORMAT_B8G8R8A8_UNORM_SRGB,
            DXGI_FORMAT_B8G8R8X8_UNORM,
        ]
        .contains(&format),
        NvencLiveInput::Rgba => {
            [DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM_SRGB].contains(&format)
        }
        NvencLiveInput::Nv12 => format == DXGI_FORMAT_NV12,
    }
}

fn device_from_handle(handle: NativeHandle) -> Result<ID3D11Device, EncodeError> {
    let raw = handle.get() as *mut c_void;
    // SAFETY: the caller passes a live `ID3D11Device*` (`NvencLiveConfig::device`).
    let borrowed =
        unsafe { ID3D11Device::from_raw_borrowed(&raw) }.ok_or(EncodeError::InvalidInput)?;
    Ok(borrowed.clone()) // clone: COM AddRef, the session keeps the device alive
}

const fn stream_info(width: u32, height: u32, cfg: &NvencLiveConfig) -> StreamInfo {
    StreamInfo::Video {
        id: 0,
        codec: CodecKind::H264,
        time_base: cfg.time_base,
        geometry: VideoGeometry { width, height },
        extra_data: Bytes::new(),
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
