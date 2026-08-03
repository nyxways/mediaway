//! NVENC H.264 / HEVC / AV1 CPU-upload encode session (`NV_ENC_DEVICE_TYPE_DIRECTX`).
//!
//! See [ADR-0001](../../adr/0001-nvenc-vendor-backend.md) 2026-07-29 addendum: bindings
//! choice (the `nvenc` crate, depended on directly), a real hardware-verified bug in that
//! crate's native `NvEncCreateInputBuffer`/lock host-memory path (worked around via
//! [`super::device::Dx11Upload`]), and this stage's scope (fixed P3/`HighQuality` preset,
//! `enable_ptd = true` automatic GOP/picture-type decisions, no explicit IDR/GOP control).
//! See the same ADR's 2026-07-29 (HEVC/AV1) addendum for per-codec bitstream framing
//! differences (H.264/HEVC are Annex-B NAL streams; AV1 is OBU-framed, no start codes) and
//! real hardware findings for HEVC and AV1 on this crate's reference RTX 4090.

use std::collections::VecDeque;

use crate::{EncodeError, VideoEncoder, VideoEncoderConfig, VideoInputPreference};
use mediaway_common::{
    Bytes, CodecKind, Packet, PixelFormat, Rational, StreamInfo, VideoFrame, VideoFrameStorage,
    VideoGeometry,
};

use nvenc::bitstream::BitStream;
use nvenc::encoder::{Encoder, RegisteredResource};
use nvenc::session::{InitParams, Session};
use nvenc::sys::enums::{
    NVencBufferFormat, NVencParamsRcMode, NVencPicStruct, NVencPicType, NVencTuningInfo,
};
use nvenc::sys::guids::{
    NV_ENC_CODEC_AV1_GUID, NV_ENC_CODEC_H264_GUID, NV_ENC_CODEC_HEVC_GUID, NV_ENC_PRESET_P3_GUID,
};
use nvenc::sys::structs::Guid;
use windows::Win32::Graphics::Direct3D11::ID3D11Device;

use super::device::{self, Dx11Upload};

/// Backend default bitrate when [`VideoEncoderConfig::bitrate_bps`] is `0` (unset) —
/// matches `mediaway-encoder-windows`'s WMF `bitrate_and_fps` default.
const DEFAULT_BITRATE_BPS: u32 = 2_000_000;

/// NVENC H.264 / HEVC / AV1 CPU-upload encode session (codec fixed at [`open`](Self::open)).
///
/// Field order is load-bearing for `Drop`: NVENC's registered/mapped resource and bitstream
/// buffer must release their references to the D3D11 textures and encoder session **before**
/// the textures (`upload`), the encode session (`encoder`), and the device are torn down —
/// Rust drops named-struct fields in declaration order.
pub(crate) struct NvencSession {
    registered: RegisteredResource,
    bitstream: BitStream,
    upload: Dx11Upload,
    encoder: Encoder,
    /// Kept alive for the session's lifetime — NVENC and the textures above were created
    /// against it; never exposed to callers (see module docs).
    _device: ID3D11Device,
    info: StreamInfo,
    width: u32,
    height: u32,
    /// Selects the keyframe-detection scan in [`push_frame`](Self::push_frame) — bitstream
    /// framing (Annex-B NAL vs. AV1 OBU) is codec-specific; see [`is_keyframe_packet`].
    codec: CodecKind,
    frame_idx: u32,
    pending: VecDeque<Packet>,
    flushed: bool,
}

impl NvencSession {
    pub(crate) fn open(config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        validate(config)?;
        let codec_guid = codec_guid(config.codec).ok_or(EncodeError::Unsupported)?;

        let (device, ctx) = device::open_device()?;
        let upload = Dx11Upload::new(&device, ctx, config.width, config.height)?;

        // The `nvenc` crate's `Session::open_dx` unwraps its internal NVENC runtime DLL
        // load (`nvenc_init()`), so a machine without the NVENC driver panics instead of
        // returning `Err`. Pre-check the DLL so `open` degrades to
        // `Err(EncodeError::Backend)` and the `_or_skip_without_hw` tests skip instead.
        if !nvenc_runtime_available() {
            return Err(EncodeError::Backend);
        }
        let session = Session::open_dx(&device).map_err(|_| EncodeError::Backend)?;
        let codecs = session
            .get_encode_codecs()
            .map_err(|_| EncodeError::Backend)?;
        if !codecs.contains(&codec_guid) {
            return Err(EncodeError::Unsupported);
        }

        let (session, mut preset_config) = session
            .get_encode_preset_config_ex(
                // clone: `get_encode_preset_config_ex` takes `codec` by value; `codec_guid`
                // is still needed below for `InitParams::encode_guid` (16-byte POD `Guid`).
                codec_guid.clone(),
                NV_ENC_PRESET_P3_GUID,
                NVencTuningInfo::HighQuality,
            )
            .map_err(|_| EncodeError::Backend)?;

        let bitrate = if config.bitrate_bps == 0 {
            DEFAULT_BITRATE_BPS
        } else {
            config.bitrate_bps
        };
        preset_config.preset_cfg.rc_params.rate_control_mode = NVencParamsRcMode::VBR;
        preset_config.preset_cfg.rc_params.average_bit_rate = bitrate;
        preset_config.preset_cfg.gop_len = 30;
        preset_config.preset_cfg.frame_interval_p = 1;

        let init_params = InitParams {
            encode_guid: codec_guid,
            preset_guid: NV_ENC_PRESET_P3_GUID,
            resolution: [config.width, config.height],
            aspect_ratio: [config.width, config.height],
            frame_rate: frame_rate(config.time_base),
            tuning_info: NVencTuningInfo::HighQuality,
            buffer_format: NVencBufferFormat::NV12,
            encode_config: &mut preset_config.preset_cfg,
            enable_ptd: true,
            max_encoder_resolution: [0, 0],
        };

        let encoder = session
            .init_encoder(init_params)
            .map_err(|_| EncodeError::Backend)?;

        let registered = encoder
            .register_resource_dx11(upload.gpu_texture(), NVencBufferFormat::NV12, 0)
            .map_err(|_| EncodeError::Backend)?;
        let bitstream = encoder
            .create_bitstream_buffer()
            .map_err(|_| EncodeError::Backend)?;

        Ok(Self {
            registered,
            bitstream,
            upload,
            encoder,
            _device: device,
            info: stream_info_from(config),
            width: config.width,
            height: config.height,
            codec: config.codec,
            frame_idx: 0,
            pending: VecDeque::new(),
            flushed: false,
        })
    }
}

/// Maps a portable [`CodecKind`] to the NVENC codec GUID this backend requests, or `None`
/// for codecs NVENC does not encode at all — e.g. VP9 (NVENC is VP9 **decode**-only, see
/// ADR-0001) — or that are not video codecs. `Some` here does not itself guarantee the
/// codec is hardware-supported on the caller's GPU/driver; `open()` still probes
/// `Session::get_encode_codecs()` before proceeding.
const fn codec_guid(codec: CodecKind) -> Option<Guid> {
    match codec {
        CodecKind::H264 => Some(NV_ENC_CODEC_H264_GUID),
        CodecKind::Hevc => Some(NV_ENC_CODEC_HEVC_GUID),
        CodecKind::Av1 => Some(NV_ENC_CODEC_AV1_GUID),
        _ => None,
    }
}

impl VideoEncoder for NvencSession {
    fn stream_info(&self) -> &StreamInfo {
        &self.info
    }

    fn push_frame(&mut self, frame: &VideoFrame) -> Result<(), EncodeError> {
        if self.flushed {
            return Err(EncodeError::Closed);
        }
        let VideoFrameStorage::Cpu { data } = &frame.storage else {
            return Err(EncodeError::Unsupported);
        };
        if frame.width != self.width || frame.height != self.height {
            return Err(EncodeError::InvalidInput);
        }

        self.upload.upload_cpu_nv12(data, self.width, self.height)?;

        let timestamp = u64::try_from(frame.pts).unwrap_or(0);
        let frame_idx = self.frame_idx;
        self.frame_idx = self.frame_idx.wrapping_add(1);

        self.encoder
            .encode_picture(
                &self.registered,
                &self.bitstream,
                frame_idx as usize,
                timestamp,
                NVencBufferFormat::NV12,
                NVencPicStruct::Frame,
                NVencPicType::UNKNOWN,
                None,
            )
            .map_err(|_| EncodeError::Backend)?;

        let lock = self
            .bitstream
            .try_lock(true)
            .map_err(|_| EncodeError::Backend)?;
        let payload = lock.as_slice();
        let is_keyframe = is_keyframe_packet(self.codec, payload);
        // `bitstream` is reused every frame (see field docs) — its locked slice is only
        // valid until `lock` drops, so the packet needs an owned copy of these bytes.
        let packet = Packet {
            stream_id: 0,
            pts: frame.pts,
            dts: frame.pts,
            duration: frame.duration,
            is_keyframe,
            is_discard: false,
            payload: Bytes::from(payload.to_vec()),
        };
        drop(lock);
        self.pending.push_back(packet);
        Ok(())
    }

    fn poll_packet(&mut self) -> Result<Option<Packet>, EncodeError> {
        Ok(self.pending.pop_front())
    }

    fn flush(&mut self) -> Result<(), EncodeError> {
        if self.flushed {
            return Ok(());
        }
        self.flushed = true;
        self.encoder
            .end_encode()
            .map_err(|_| EncodeError::Backend)?;
        Ok(())
    }
}

/// `[fps_num, fps_den]` from a `num/den`-seconds [`Rational`] timebase (`fps = den/num`) —
/// same convention as `mediaway-encoder-windows`'s WMF `bitrate_and_fps` helper.
fn frame_rate(time_base: Rational) -> [u32; 2] {
    let fps_num = time_base.den;
    let fps_den = u32::try_from(time_base.num.max(1)).unwrap_or(1);
    [fps_num, fps_den]
}

/// Codec-specific keyframe/sync-point detection, dispatched by `codec` — bitstream framing
/// differs per codec (H.264/HEVC are Annex-B NAL streams; AV1 is OBU-framed with no start
/// codes at all), so each codec gets its own scan. See [`contains_h264_idr_nal`],
/// [`contains_hevc_idr_nal`], [`contains_av1_sequence_header_obu`].
fn is_keyframe_packet(codec: CodecKind, data: &[u8]) -> bool {
    match codec {
        CodecKind::H264 => contains_h264_idr_nal(data),
        CodecKind::Hevc => contains_hevc_idr_nal(data),
        CodecKind::Av1 => contains_av1_sequence_header_obu(data),
        // Unreachable in practice: `validate()` (via `codec_guid`) rejects every other
        // `CodecKind` before a session can open.
        _ => false,
    }
}

/// Whether Annex-B `data` contains an H.264 IDR slice NAL (type 5, 4-byte start code
/// `00 00 00 01` — the shape NVENC always emits). This stage's packets are small
/// (single-slice-per-frame, one NVENC bitstream buffer drained per `push_frame`), so a
/// linear scan is fine here — not a hot per-sample loop.
fn contains_h264_idr_nal(data: &[u8]) -> bool {
    data.windows(5)
        .any(|w| w[..4] == [0, 0, 0, 1] && (w[4] & 0x1F) == 5)
}

/// Whether Annex-B `data` contains an HEVC IDR slice NAL (type 19 `IDR_W_RADL` or type 20
/// `IDR_N_LP`) after a 4-byte start code. HEVC NAL headers are 2 bytes
/// (`forbidden_zero_bit(1) + nal_unit_type(6) + nuh_layer_id(6) + nuh_temporal_id_plus1(3)`),
/// unlike H.264's 1-byte header — the type sits in the top 6 bits of the first header byte,
/// i.e. `(byte >> 1) & 0x3F`. Same linear-scan cost note as [`contains_h264_idr_nal`].
fn contains_hevc_idr_nal(data: &[u8]) -> bool {
    data.windows(5)
        .any(|w| w[..4] == [0, 0, 0, 1] && matches!((w[4] >> 1) & 0x3F, 19 | 20))
}

/// Whether OBU-framed AV1 `data` contains an `OBU_SEQUENCE_HEADER` (`obu_type == 1`).
/// AV1 has no NAL/Annex-B start codes; NVENC emits the sequence header OBU only on/before a
/// keyframe (same convention every AV1 encoder follows), so its presence is this backend's
/// keyframe signal for AV1. Walks OBUs via each header's `obu_size` `leb128` field
/// ([`read_leb128`]) to skip to the next OBU; stops (treating whatever was found so far as
/// the answer) at a malformed header, an OBU without a size field (this backend always
/// requests OBUs with a size field — an unset bit means unexpected data, not a shape this
/// scan needs to walk past), or a truncated buffer.
fn contains_av1_sequence_header_obu(data: &[u8]) -> bool {
    let mut i = 0usize;
    while i < data.len() {
        let header = data[i];
        if header & 0x80 != 0 {
            break; // forbidden_bit set — not a valid OBU header.
        }
        let obu_type = (header >> 3) & 0x0F;
        let has_extension = header & 0x04 != 0;
        let has_size_field = header & 0x02 != 0;
        let mut pos = i + 1;
        if has_extension {
            pos += 1;
        }
        if !has_size_field {
            break;
        }
        let Some((obu_size, leb_len)) = read_leb128(data.get(pos..).unwrap_or_default()) else {
            break;
        };
        if obu_type == 1 {
            return true;
        }
        pos += leb_len;
        let Some(next) = pos.checked_add(obu_size) else {
            break;
        };
        i = next;
    }
    false
}

/// Minimal AV1 `leb128` reader (little-endian base-128, used by AV1's `obu_size` field) —
/// returns `(value, bytes_consumed)`. Capped at 8 bytes: the AV1 spec limits `leb128` to
/// values fitting `u64`, and no field this backend reads needs more.
fn read_leb128(data: &[u8]) -> Option<(usize, usize)> {
    let mut value: u64 = 0;
    for (i, &byte) in data.iter().take(8).enumerate() {
        value |= u64::from(byte & 0x7F) << (i * 7);
        if byte & 0x80 == 0 {
            return Some((usize::try_from(value).ok()?, i + 1));
        }
    }
    None
}

fn validate(config: &VideoEncoderConfig) -> Result<(), EncodeError> {
    if codec_guid(config.codec).is_none() {
        return Err(EncodeError::Unsupported);
    }
    if config.input != VideoInputPreference::CpuUploadOk {
        // ZeroCopyGpu (caller-supplied D3D11/D3D12 texture) is deferred — see module docs.
        return Err(EncodeError::Unsupported);
    }
    if config.width == 0 || config.height == 0 || config.width % 2 != 0 || config.height % 2 != 0 {
        return Err(EncodeError::InvalidInput);
    }
    if config.pixel_format != PixelFormat::Nv12 {
        return Err(EncodeError::Unsupported);
    }
    if config.time_base.den == 0 {
        return Err(EncodeError::InvalidInput);
    }
    Ok(())
}

#[allow(clippy::missing_const_for_fn, reason = "StreamInfo holds Bytes")]
fn stream_info_from(config: &VideoEncoderConfig) -> StreamInfo {
    StreamInfo::Video {
        id: 0,
        codec: config.codec,
        time_base: config.time_base,
        geometry: VideoGeometry {
            width: config.width,
            height: config.height,
        },
        extra_data: Bytes::new(),
    }
}

/// Whether the NVENC runtime DLL can be loaded — mirrors the `nvenc` crate's `NVENC_DLL`
/// constant (64-bit host). The crate itself unwraps this load internally, so the probe
/// here is what keeps [`NvencSession::open`] (and the `_or_skip_without_hw` tests) honest
/// on machines without the NVENC driver.
fn nvenc_runtime_available() -> bool {
    // SAFETY: probes a well-known driver DLL by name; the returned module handle is
    // dropped immediately (the load itself is all we check). No other effect.
    unsafe {
        windows::Win32::System::LibraryLoader::LoadLibraryExW(
            windows::core::w!("nvEncodeAPI64.dll"),
            None,
            windows::Win32::System::LibraryLoader::LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
        )
        .is_ok()
    }
}

#[cfg(test)]
#[path = "video_tests.rs"]
mod tests;
