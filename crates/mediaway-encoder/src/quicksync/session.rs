//! Windows Quick Sync / Arc (oneVPL) H.264/HEVC/AV1 CPU-upload encode session.
//!
//! Stage 1 scope: [`VideoInputPreference::CpuUploadOk`] only (NV12,
//! `upload_cpu_nv12` — a real per-frame CPU copy, documented on that method).
//! [`VideoInputPreference::ZeroCopyGpu`] is not implemented here; see this
//! crate's `adr/0001-onevpl-quicksync-encode-surface.md` for the deferred
//! D3D11 external-allocator design.
//!
//! Codec: [`CodecKind::H264`] (Baseline) and [`CodecKind::Hevc`] (Main) are
//! real, hardware-verified encode paths on this workspace's Intel UHD 770.
//! [`CodecKind::Av1`] is accepted by [`codec_params`]/[`validate`] and
//! attempted honestly through the same `MFXVideoENCODE_Query`/`_Init` path —
//! this generation of Intel iGPU (Alder Lake / Xe-LP) is not documented as
//! supporting AV1 hardware *encode* (AV1 *decode* is a separate capability),
//! so it is expected to surface as [`EncodeError::Unsupported`] /
//! [`EncodeError::Backend`] here; see `session_tests.rs`'s dedicated
//! diagnostic test for the real captured `mfxStatus` and this crate's
//! `adr/0001` 2026-07-29 HEVC/AV1 addendum for the exact result.
//!
//! GOP: real I/P structure (`GopPicSize`/`GopRefDist`, driver-managed
//! reference lists) — unlike the Linux VA-API backend's all-IDR stage, this
//! crate's `MFXVideoENCODE_EncodeFrameAsync` flow lets the oneVPL runtime own
//! GOP/reference-list bookkeeping directly (frames are submitted in display
//! order; the driver assigns I/P types itself), so display order equals
//! encode order and `pts == dts` on every emitted [`Packet`].

#![allow(unsafe_code)]
// `pub(crate)` (not `pub`) satisfies the workspace's `unreachable_pub` rustc
// lint (nothing in this private module is reachable outside the crate);
// clippy's `redundant_pub_crate` disagrees and recommends plain `pub` for the
// same shape — a known lint conflict with no spelling that satisfies both, so
// the rustc lint (an explicit workspace choice, `docs/conventions`) wins.
#![allow(clippy::redundant_pub_crate)]

use std::collections::VecDeque;
use std::time::Duration;

use crate::{EncodeError, VideoEncoder, VideoEncoderConfig, VideoInputPreference};
use mediaway_common::{
    Bytes, CodecKind, Packet, PixelFormat, Rational, StreamInfo, VideoFrame, VideoFrameStorage,
    VideoGeometry,
};

use vpl_sys::consts::{
    MFX_CHROMAFORMAT_YUV420, MFX_CODEC_AV1, MFX_CODEC_AVC, MFX_CODEC_HEVC, MFX_ERR_MORE_DATA,
    MFX_ERR_NONE, MFX_FOURCC_NV12, MFX_FRAMETYPE_IDR, MFX_IMPL_HARDWARE_ANY,
    MFX_IOPATTERN_IN_SYSTEM_MEMORY, MFX_LEVEL_AV1_41, MFX_LEVEL_AVC_41, MFX_LEVEL_HEVC_41,
    MFX_PICSTRUCT_PROGRESSIVE, MFX_PROFILE_AV1_MAIN, MFX_PROFILE_AVC_BASELINE,
    MFX_PROFILE_HEVC_MAIN, MFX_RATECONTROL_CBR, MFX_RATECONTROL_CQP, MFX_TARGETUSAGE_BALANCED,
    MFX_WRN_DEVICE_BUSY,
};
use vpl_sys::raw::{
    mfxBitstream, mfxFrameData__bindgen_ty_2, mfxFrameData__bindgen_ty_3,
    mfxFrameData__bindgen_ty_4, mfxFrameInfo, mfxFrameInfo__bindgen_ty_1,
    mfxFrameInfo__bindgen_ty_1__bindgen_ty_1, mfxFrameSurface1, mfxInfoMFX,
    mfxInfoMFX__bindgen_ty_1, mfxInfoMFX__bindgen_ty_1__bindgen_ty_1,
    mfxInfoMFX__bindgen_ty_1__bindgen_ty_1__bindgen_ty_1,
    mfxInfoMFX__bindgen_ty_1__bindgen_ty_1__bindgen_ty_2,
    mfxInfoMFX__bindgen_ty_1__bindgen_ty_1__bindgen_ty_3, mfxVideoParam,
    mfxVideoParam__bindgen_ty_1,
};
use vpl_sys::{Loader, Session};

/// `MFXVideoENCODE_EncodeFrameAsync` retries on `MFX_WRN_DEVICE_BUSY` before
/// giving up (each retry sleeps [`DEVICE_BUSY_RETRY_DELAY`]).
const DEVICE_BUSY_RETRY_LIMIT: u32 = 50;
/// Sleep between `MFX_WRN_DEVICE_BUSY` retries.
const DEVICE_BUSY_RETRY_DELAY: Duration = Duration::from_millis(2);
/// `MFXVideoCORE_SyncOperation` wait timeout per completed submission.
const SYNC_WAIT_MS: u32 = 60_000;

/// Windows Quick Sync / Arc H.264 CPU-upload encode session (real oneVPL backend).
pub(crate) struct QuickSyncSession {
    session: Session,
    info: StreamInfo,
    frame_info: mfxFrameInfo,
    width: u32,
    height: u32,
    aligned_width: u32,
    aligned_height: u32,
    time_base: Rational,
    /// Reusable NV12 upload buffer (`aligned_width * aligned_height * 3 / 2`
    /// bytes), sized once at [`open`](Self::open) and never reallocated —
    /// see [`Self::upload_cpu_nv12`].
    upload_buf: Vec<u8>,
    /// Reusable output bitstream buffer, `Data` points into `bitstream_storage`.
    bitstream: mfxBitstream,
    /// Backing allocation for `bitstream.Data` — never read directly (only
    /// through that raw pointer in `collect_packet`), kept alive purely for
    /// RAII ownership of the buffer `bitstream.Data` aliases into.
    #[allow(
        dead_code,
        reason = "kept alive for bitstream.Data's backing allocation"
    )]
    bitstream_storage: Vec<u8>,
    pending: VecDeque<Packet>,
    flushed: bool,
}

impl QuickSyncSession {
    /// Open a real oneVPL H.264 CPU-upload encode session against
    /// [`vpl_sys::consts::MFX_IMPL_HARDWARE_ANY`] (see that constant's docs
    /// for why not the narrower `MFX_IMPL_HARDWARE`).
    ///
    /// # Errors
    ///
    /// - [`EncodeError::Unsupported`] — codec/pixel-format/input path outside
    ///   this stage's scope, or `MFXVideoENCODE_Query` rejected the params.
    /// - [`EncodeError::InvalidInput`] — zero/odd/oversized dimensions or a
    ///   zero timebase denominator.
    /// - [`EncodeError::Backend`] — no oneVPL runtime found, session/encoder
    ///   init failed.
    pub(crate) fn open(config: &VideoEncoderConfig) -> Result<Self, EncodeError> {
        validate(config)?;

        let loader = Loader::open().map_err(|_| EncodeError::Backend)?;
        let mut session = loader
            .create_session(MFX_IMPL_HARDWARE_ANY)
            .map_err(|_| EncodeError::Backend)?;

        let (codec_id, codec_profile, codec_level) = codec_params(config.codec)?;
        let aligned_width = align16(config.width);
        let aligned_height = align16(config.height);
        let frame_info = build_frame_info(config, aligned_width, aligned_height);
        let mfx = build_mfx_info(
            frame_info,
            config.bitrate_bps,
            codec_id,
            codec_profile,
            codec_level,
        );
        let mut params = mfxVideoParam {
            AsyncDepth: 1,
            __bindgen_anon_1: mfxVideoParam__bindgen_ty_1 { mfx },
            IOPattern: MFX_IOPATTERN_IN_SYSTEM_MEMORY,
            ..Default::default()
        };

        session
            .encode_query(&mut params)
            .map_err(|_| EncodeError::Unsupported)?;
        session
            .encode_init(&mut params)
            .map_err(|_| EncodeError::Backend)?;

        let plane_pixels = (aligned_width as usize) * (aligned_height as usize);
        let upload_buf = vec![0u8; plane_pixels + plane_pixels / 2];

        // `mfxBitstream::MaxLength` must be at least the encoder's own
        // configured `mfxInfoMFX::BufferSizeInKB` (VBV buffer size in KB, set
        // in `build_mfx_info` from `bitrate_bps`) or
        // `MFXVideoENCODE_EncodeFrameAsync` returns `MFX_ERR_NOT_ENOUGH_BUFFER`
        // — hardware-confirmed on this workspace's Intel UHD 770 (a plain
        // frame-sized buffer was rejected at any nonzero `bitrate_bps` /CBR
        // path). `* 1000` matches oneVPL's own KB definition ("in this
        // context, KB is 1000 bytes" — `mfxstructures.h`).
        let bitrate_buffer_bytes = if config.bitrate_bps == 0 {
            0
        } else {
            ((config.bitrate_bps / 1000).max(1) as usize) * 2 * 1000
        };
        let bitstream_capacity = (plane_pixels + plane_pixels / 2)
            .max(bitrate_buffer_bytes)
            .max(1 << 16);
        let mut bitstream_storage = vec![0u8; bitstream_capacity];
        let bitstream = mfxBitstream {
            Data: bitstream_storage.as_mut_ptr(),
            MaxLength: u32::try_from(bitstream_capacity).unwrap_or(u32::MAX),
            ..Default::default()
        };

        Ok(Self {
            session,
            info: stream_info_from(config),
            frame_info,
            width: config.width,
            height: config.height,
            aligned_width,
            aligned_height,
            time_base: config.time_base,
            upload_buf,
            bitstream,
            bitstream_storage,
            pending: VecDeque::new(),
            flushed: false,
        })
    }

    /// Copy `frame`'s tightly-packed NV12 bytes into [`Self::upload_buf`],
    /// which is padded/strided to `aligned_width`/`aligned_height` (oneVPL's
    /// documented minimum-multiple-of-16 surface alignment) — a genuine
    /// CPU-side copy, not Zero-Copy, matching this workspace's
    /// `upload_cpu_nv12` naming/cost-disclosure convention
    /// (`mediaway-encoder-windows`/`mediaway-encoder-linux`).
    fn upload_cpu_nv12(&mut self, frame: &VideoFrame) -> Result<(), EncodeError> {
        let VideoFrameStorage::Cpu { data } = &frame.storage else {
            return Err(EncodeError::Unsupported);
        };
        if frame.width != self.width || frame.height != self.height {
            return Err(EncodeError::InvalidInput);
        }
        let width = self.width as usize;
        let height = self.height as usize;
        let src_y_size = width * height;
        let src_uv_size = src_y_size / 2;
        if data.len() < src_y_size + src_uv_size {
            return Err(EncodeError::InvalidInput);
        }

        let aligned_width = self.aligned_width as usize;
        for row in 0..height {
            let src_off = row * width;
            let dst_off = row * aligned_width;
            self.upload_buf[dst_off..dst_off + width]
                .copy_from_slice(&data[src_off..src_off + width]);
        }
        let uv_plane_dst = aligned_width * (self.aligned_height as usize);
        let uv_rows = height / 2;
        for row in 0..uv_rows {
            let src_off = src_y_size + row * width;
            let dst_off = uv_plane_dst + row * aligned_width;
            self.upload_buf[dst_off..dst_off + width]
                .copy_from_slice(&data[src_off..src_off + width]);
        }
        Ok(())
    }

    /// Build the `mfxFrameSurface1` for the just-uploaded [`Self::upload_buf`]
    /// and submit it (`MFXVideoENCODE_EncodeFrameAsync`, retrying on
    /// `MFX_WRN_DEVICE_BUSY`), syncing and collecting a packet on
    /// `MFX_ERR_NONE`. `surface_val = None` signals end-of-stream (flush
    /// drain). Returns whether a packet was produced this call.
    fn encode_and_collect(
        &mut self,
        mut surface_val: Option<mfxFrameSurface1>,
    ) -> Result<bool, EncodeError> {
        let mut retries = 0u32;
        loop {
            self.bitstream.DataOffset = 0;
            self.bitstream.DataLength = 0;
            let (status, syncp) = self
                .session
                .encode_frame_async(surface_val.as_mut(), &mut self.bitstream)
                .map_err(|_| EncodeError::Backend)?;

            if status == MFX_WRN_DEVICE_BUSY {
                retries += 1;
                if retries > DEVICE_BUSY_RETRY_LIMIT {
                    return Err(EncodeError::Backend);
                }
                std::thread::sleep(DEVICE_BUSY_RETRY_DELAY);
                continue;
            }
            if status == MFX_ERR_NONE {
                self.session
                    .sync_operation(syncp, SYNC_WAIT_MS)
                    .map_err(|_| EncodeError::Backend)?;
                self.collect_packet();
                return Ok(true);
            }
            if status == MFX_ERR_MORE_DATA {
                return Ok(false);
            }
            if status < MFX_ERR_NONE {
                return Err(EncodeError::Backend);
            }
            // Any other non-negative status (a warning this crate does not
            // specifically special-case): treat as "no packet this call",
            // never as a hard failure — `mfx_succeeded` already covers this
            // family at the `vpl-sys` layer.
            return Ok(false);
        }
    }

    /// Read the bitstream `MFXVideoENCODE_EncodeFrameAsync` + `SyncOperation`
    /// just wrote (`DataOffset..DataOffset+DataLength` into
    /// [`Self::bitstream_storage`]) into an owned [`Packet`], then reset the
    /// offsets so the same buffer is reused next call.
    fn collect_packet(&mut self) {
        let offset = self.bitstream.DataOffset as usize;
        let len = self.bitstream.DataLength as usize;
        if len == 0 {
            return;
        }
        // SAFETY: `self.bitstream.Data` was set at `open()` to
        // `self.bitstream_storage.as_mut_ptr()` and never reassigned;
        // `bitstream_storage` is never reallocated after `open()` (no
        // push/resize), so the pointer stays valid for `self`'s lifetime.
        // `offset`/`len` describe the sub-range `MFXVideoENCODE_EncodeFrameAsync`
        // just filled, checked non-zero above and only reached after a
        // `MFX_ERR_NONE` + successful `MFXVideoCORE_SyncOperation` in
        // `encode_and_collect`, per oneVPL's documented output contract.
        let bytes = unsafe { std::slice::from_raw_parts(self.bitstream.Data.add(offset), len) };
        // Necessary copy: `bitstream_storage` is reused (overwritten) by the
        // next `encode_and_collect` call, so the packet must own independent
        // bytes rather than borrow this session's buffer.
        let payload = Bytes::copy_from_slice(bytes);
        let is_idr = (u32::from(self.bitstream.FrameType) & u32::from(MFX_FRAMETYPE_IDR)) != 0;
        let pts = ts_from_90k(self.time_base, self.bitstream.TimeStamp);

        self.pending.push_back(Packet {
            stream_id: 0,
            pts,
            dts: pts,
            duration: 0,
            is_keyframe: is_idr,
            is_discard: false,
            payload,
        });

        self.bitstream.DataOffset = 0;
        self.bitstream.DataLength = 0;
    }
}

impl VideoEncoder for QuickSyncSession {
    fn stream_info(&self) -> &StreamInfo {
        &self.info
    }

    fn push_frame(&mut self, frame: &VideoFrame) -> Result<(), EncodeError> {
        if self.flushed {
            return Err(EncodeError::Closed);
        }
        self.upload_cpu_nv12(frame)?;

        let uv_offset = (self.aligned_width as usize) * (self.aligned_height as usize);
        let (y_part, uv_part) = self.upload_buf.split_at_mut(uv_offset);

        let mut surface = mfxFrameSurface1 {
            Info: self.frame_info,
            ..Default::default()
        };
        surface.Data.__bindgen_anon_2 = mfxFrameData__bindgen_ty_2 {
            Pitch: u16::try_from(self.aligned_width).unwrap_or(u16::MAX),
        };
        surface.Data.__bindgen_anon_3 = mfxFrameData__bindgen_ty_3 {
            Y: y_part.as_mut_ptr(),
        };
        surface.Data.__bindgen_anon_4 = mfxFrameData__bindgen_ty_4 {
            UV: uv_part.as_mut_ptr(),
        };
        surface.Data.TimeStamp = ts_to_90k(self.time_base, frame.pts);

        self.encode_and_collect(Some(surface))?;
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
        while self.encode_and_collect(None)? {}
        Ok(())
    }
}

impl Drop for QuickSyncSession {
    fn drop(&mut self) {
        // Teardown order matches oneVPL's documented contract
        // (`MFXVideoENCODE_Close` before session close); ignoring the status
        // matches every other Windows/Linux backend's teardown-`Drop`
        // convention in this workspace (no `unwrap`/`panic!` in a
        // destructor). `self.session`'s own `Drop` (`MFXClose`) runs
        // immediately after this fn returns.
        let _ = self.session.encode_close();
    }
}

fn validate(config: &VideoEncoderConfig) -> Result<(), EncodeError> {
    if !matches!(
        config.codec,
        CodecKind::H264 | CodecKind::Hevc | CodecKind::Av1
    ) {
        return Err(EncodeError::Unsupported);
    }
    if !matches!(config.input, VideoInputPreference::CpuUploadOk) {
        // ZeroCopyGpu (D3D11 external-allocator surfaces) is future work —
        // see this crate's adr/0001.
        return Err(EncodeError::Unsupported);
    }
    if config.pixel_format != PixelFormat::Nv12 {
        // CPU-upload path is NV12-only, matching the Windows WMF / Linux
        // VA-API backends' `upload_cpu_nv12` convention.
        return Err(EncodeError::Unsupported);
    }
    if config.width == 0
        || config.height == 0
        || config.width % 2 != 0
        || config.height % 2 != 0
        || config.width > u32::from(u16::MAX)
        || config.height > u32::from(u16::MAX)
    {
        return Err(EncodeError::InvalidInput);
    }
    if config.time_base.den == 0 {
        return Err(EncodeError::InvalidInput);
    }
    Ok(())
}

const fn align16(x: u32) -> u32 {
    (x + 15) & !15
}

/// oneVPL `(CodecId, CodecProfile, CodecLevel)` for a [`CodecKind`] this crate
/// attempts. [`validate`] only lets [`CodecKind::H264`]/[`CodecKind::Hevc`]/
/// [`CodecKind::Av1`] reach here, so every other variant is unreachable by
/// construction — this stays a `Result` (not an infallible match) so a future
/// codec added to `validate` without a matching arm here fails loudly instead
/// of silently defaulting to an unrelated codec's parameters.
///
/// [`CodecKind::Hevc`] is a real, hardware-verified encode path on this
/// workspace's Intel UHD 770 (see `adr/0001`'s 2026-07-29 HEVC/AV1 addendum).
/// [`CodecKind::Av1`] is honestly attempted through the exact same
/// `MFXVideoENCODE_Query`/`_Init` calls — Alder Lake / Xe-LP is not
/// documented as supporting AV1 hardware *encode*, so callers should expect
/// [`EncodeError::Unsupported`]/[`EncodeError::Backend`] here, not a silent
/// software fallback (this crate has none); see that same addendum for the
/// real captured `mfxStatus`.
const fn codec_params(codec: CodecKind) -> Result<(u32, u16, u16), EncodeError> {
    match codec {
        CodecKind::H264 => Ok((MFX_CODEC_AVC, MFX_PROFILE_AVC_BASELINE, MFX_LEVEL_AVC_41)),
        CodecKind::Hevc => Ok((MFX_CODEC_HEVC, MFX_PROFILE_HEVC_MAIN, MFX_LEVEL_HEVC_41)),
        CodecKind::Av1 => Ok((MFX_CODEC_AV1, MFX_PROFILE_AV1_MAIN, MFX_LEVEL_AV1_41)),
        _ => Err(EncodeError::Unsupported),
    }
}

fn build_frame_info(
    config: &VideoEncoderConfig,
    aligned_width: u32,
    aligned_height: u32,
) -> mfxFrameInfo {
    let width = u16::try_from(aligned_width).unwrap_or(u16::MAX);
    let height = u16::try_from(aligned_height).unwrap_or(u16::MAX);
    let crop_w = u16::try_from(config.width).unwrap_or(u16::MAX);
    let crop_h = u16::try_from(config.height).unwrap_or(u16::MAX);
    mfxFrameInfo {
        FourCC: MFX_FOURCC_NV12,
        __bindgen_anon_1: mfxFrameInfo__bindgen_ty_1 {
            __bindgen_anon_1: mfxFrameInfo__bindgen_ty_1__bindgen_ty_1 {
                Width: width,
                Height: height,
                CropX: 0,
                CropY: 0,
                CropW: crop_w,
                CropH: crop_h,
            },
        },
        FrameRateExtN: config.time_base.den,
        FrameRateExtD: u32::try_from(config.time_base.num).unwrap_or(1).max(1),
        PicStruct: MFX_PICSTRUCT_PROGRESSIVE,
        ChromaFormat: MFX_CHROMAFORMAT_YUV420,
        ..Default::default()
    }
}

/// A fixed 1-second GOP at a nominal 30fps target — this stage does not yet
/// derive GOP size from the caller's actual frame rate.
const GOP_PIC_SIZE: u16 = 30;

fn build_mfx_info(
    frame_info: mfxFrameInfo,
    bitrate_bps: u32,
    codec_id: u32,
    codec_profile: u16,
    codec_level: u16,
) -> mfxInfoMFX {
    let encoding_opts = if bitrate_bps == 0 {
        mfxInfoMFX__bindgen_ty_1__bindgen_ty_1 {
            TargetUsage: MFX_TARGETUSAGE_BALANCED,
            GopPicSize: GOP_PIC_SIZE,
            GopRefDist: 1,
            RateControlMethod: MFX_RATECONTROL_CQP,
            __bindgen_anon_1: mfxInfoMFX__bindgen_ty_1__bindgen_ty_1__bindgen_ty_1 { QPI: 26 },
            __bindgen_anon_2: mfxInfoMFX__bindgen_ty_1__bindgen_ty_1__bindgen_ty_2 { QPP: 28 },
            __bindgen_anon_3: mfxInfoMFX__bindgen_ty_1__bindgen_ty_1__bindgen_ty_3 { QPB: 30 },
            NumRefFrame: 1,
            ..Default::default()
        }
    } else {
        let kbps = u16::try_from((bitrate_bps / 1000).max(1)).unwrap_or(u16::MAX);
        mfxInfoMFX__bindgen_ty_1__bindgen_ty_1 {
            TargetUsage: MFX_TARGETUSAGE_BALANCED,
            GopPicSize: GOP_PIC_SIZE,
            GopRefDist: 1,
            RateControlMethod: MFX_RATECONTROL_CBR,
            __bindgen_anon_1: mfxInfoMFX__bindgen_ty_1__bindgen_ty_1__bindgen_ty_1 {
                InitialDelayInKB: kbps,
            },
            BufferSizeInKB: kbps.saturating_mul(2),
            __bindgen_anon_2: mfxInfoMFX__bindgen_ty_1__bindgen_ty_1__bindgen_ty_2 {
                TargetKbps: kbps,
            },
            __bindgen_anon_3: mfxInfoMFX__bindgen_ty_1__bindgen_ty_1__bindgen_ty_3 {
                MaxKbps: kbps,
            },
            NumRefFrame: 1,
            ..Default::default()
        }
    };

    mfxInfoMFX {
        FrameInfo: frame_info,
        CodecId: codec_id,
        CodecProfile: codec_profile,
        CodecLevel: codec_level,
        __bindgen_anon_1: mfxInfoMFX__bindgen_ty_1 {
            __bindgen_anon_1: encoding_opts,
        },
        ..Default::default()
    }
}

/// `units` (in `time_base`) -> 90kHz ticks (oneVPL's `TimeStamp` unit, per
/// `mfxFrameData`/`mfxBitstream` docs).
fn ts_to_90k(time_base: Rational, units: i64) -> u64 {
    let num = i128::from(time_base.num);
    let den = i128::from(time_base.den.max(1));
    let ticks = i128::from(units) * 90_000 * num / den;
    u64::try_from(ticks.max(0)).unwrap_or(0)
}

/// Inverse of [`ts_to_90k`].
fn ts_from_90k(time_base: Rational, ticks90k: u64) -> i64 {
    let num = i128::from(time_base.num.max(1));
    let den = i128::from(time_base.den);
    let units = i128::from(ticks90k) * den / (90_000 * num);
    i64::try_from(units).unwrap_or(0)
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

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
