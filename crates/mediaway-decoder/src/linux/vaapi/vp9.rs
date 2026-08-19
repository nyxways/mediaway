//! VA-API VP9 decode session — `KEY_FRAME` + general single-tile `INTER_FRAME` decode
//! (compound prediction included, no artificial reference-count restriction), Profile 0
//! (8-bit 4:2:0), CPU NV12 output.
//!
//! See [ADR-0004](../adr/linux/0004-vaapi-vp9-key-frame-and-inter-decode.md) for scope, the
//! real spec-text-derived bitstream parser (Addendum, confirmed via `pdftotext` this session),
//! this ADR's own two central structural findings (VP9's entropy adaptation is driver-internal;
//! its reference model is a flat, spec-fixed 8-slot array needing only a two-field-per-slot
//! shadow table — see [`vp9::ref_table`]), and the **zero real-hardware verification** caveat
//! this backend carries the same as its H.264/AV1 siblings.

use std::collections::VecDeque;
use std::rc::Rc;

use crate::{DecodeError, VideoDecoder, VideoDecoderConfig, VideoOutputPreference};
use mediaway_common::{
    Bytes, Packet, PixelFormat, StreamInfo, VideoFrame, VideoFrameStorage, VideoGeometry,
};

use cros_libva::{
    BufferType, Config, Context, Display, Picture, PictureParameter, PictureParameterBufferVP9,
    SegmentParameterVP9, SliceParameter, SliceParameterBufferVP9, Surface, VA_FOURCC_NV12,
    VA_INVALID_ID, VA_RT_FORMAT_YUV420, VAConfigAttrib, VAConfigAttribType, VAEntrypoint,
    VAImageFormat, VP9PicFields, VP9SegmentFlags,
};

use super::codec::vp9_profile_candidates;
use super::nv12::copy_nv12_from_planes;

mod bits;
mod color_config;
mod frame_size;
mod header;
mod loop_filter;
mod quantization;
mod ref_table;
mod segmentation;
mod tile_info;

use header::Header;
use ref_table::{POOL_SIZE, RefTable, VP9_REF_SLOTS};

/// A decode pipeline bound to one negotiated coded resolution, created lazily the first time a
/// `KEY_FRAME` header is available. Unlike this crate's H.264 `Pipeline` (dynamically sized
/// `max_num_ref_frames + 1`) or its AV1 sibling (single surface, no DPB at all), this pool is a
/// **fixed** `POOL_SIZE` (`VP9_REF_SLOTS + 1`) — VP9's `RefFrameMap` size is a hard spec
/// constant, never stream-derived, so this crate needs no per-session sizing computation at all.
struct Vp9Pipeline {
    /// Kept alive for the context's lifetime — mirrors this crate's H.264/AV1 `_config` field.
    _config: Config,
    context: Rc<Context>,
    /// Physical surface pool, indexed by [`ref_table::RefEntry::pool_index`] — decoupled from
    /// VP9's *logical* reference slots (see [`ref_table`]'s module doc for why).
    surfaces: Vec<Option<Surface<()>>>,
    coded_width: u32,
    coded_height: u32,
    nv12_format: VAImageFormat,
}

/// VA-API VP9 decode session. See module docs for scope.
pub(crate) struct VaapiVp9Decoder {
    display: Rc<Display>,
    pipeline: Option<Vp9Pipeline>,
    /// This session's persistent 8-logical-slot shadow table — lives outside [`Vp9Pipeline`] so
    /// it is always well-defined (all-empty) even before the first `KEY_FRAME` creates a
    /// pipeline, matching [`VaapiAv1Decoder`](super::av1::VaapiAv1Decoder)'s `seq: Option<_>`
    /// field outliving its own `Av1Pipeline`.
    ref_table: RefTable,
    info: StreamInfo,
    declared_width: u32,
    declared_height: u32,
    pending: VecDeque<VideoFrame>,
    flushed: bool,
}

impl VaapiVp9Decoder {
    /// Open per [`VideoDecoderConfig::output`].
    pub(crate) fn open(config: &VideoDecoderConfig) -> Result<Self, DecodeError> {
        validate(config)?;
        if config.output != VideoOutputPreference::CpuFramesOk {
            // Zero-Copy DMA-BUF export deferred — see ADR-0001 § Scope (this crate's own
            // standing caveat, unchanged by this ADR).
            return Err(DecodeError::Unsupported);
        }

        let display: Rc<Display> = Display::open().ok_or(DecodeError::Unsupported)?;

        Ok(Self {
            display,
            pipeline: None,
            ref_table: RefTable::new(),
            info: stream_info_from(config),
            declared_width: config.width,
            declared_height: config.height,
            pending: VecDeque::new(),
            flushed: false,
        })
    }

    /// Create the `Config`/`Surface`/`Context` pipeline on first use. A no-op once created —
    /// dynamic resolution renegotiation mid-session is unsupported this session (matches this
    /// crate's H.264/AV1 `ensure_pipeline`).
    fn ensure_pipeline(&mut self, coded_width: u32, coded_height: u32) -> Result<(), DecodeError> {
        if self.pipeline.is_some() {
            return Ok(());
        }
        if coded_width == 0 || coded_height == 0 {
            return Err(DecodeError::InvalidInput);
        }
        // VP9's own smallest coding-unit granularity (8x8 mode-info units) — analogous to this
        // crate's AV1 sibling's `round_up_16` mb-alignment tolerance.
        if self.declared_width != 0
            && self.declared_height != 0
            && (coded_width > round_up_8(self.declared_width)
                || coded_height > round_up_8(self.declared_height))
        {
            return Err(DecodeError::InvalidInput);
        }

        let candidates = vp9_profile_candidates();
        let supported = self
            .display
            .query_config_profiles()
            .map_err(|_| DecodeError::Backend)?;
        let mut chosen = None;
        for profile in candidates {
            if !supported.contains(&profile) {
                continue;
            }
            let entrypoints = self
                .display
                .query_config_entrypoints(profile)
                .map_err(|_| DecodeError::Backend)?;
            if entrypoints.contains(&VAEntrypoint::VAEntrypointVLD) {
                chosen = Some(profile);
                break;
            }
        }
        let profile = chosen.ok_or(DecodeError::Unsupported)?;

        let attrs = vec![VAConfigAttrib {
            type_: VAConfigAttribType::VAConfigAttribRTFormat,
            value: VA_RT_FORMAT_YUV420,
        }];
        let config = self
            .display
            .create_config(attrs, profile, VAEntrypoint::VAEntrypointVLD)
            .map_err(|_| DecodeError::Backend)?;

        let surfaces = self
            .display
            .create_surfaces(
                VA_RT_FORMAT_YUV420,
                Some(VA_FOURCC_NV12),
                coded_width,
                coded_height,
                None,
                vec![(); POOL_SIZE],
            )
            .map_err(|_| DecodeError::Backend)?;

        let context = self
            .display
            .create_context(&config, coded_width, coded_height, Some(&surfaces), true)
            .map_err(|_| DecodeError::Backend)?;

        let nv12_format = self
            .display
            .query_image_formats()
            .map_err(|_| DecodeError::Backend)?
            .into_iter()
            .find(|f| f.fourcc == VA_FOURCC_NV12)
            .ok_or(DecodeError::Unsupported)?;

        self.pipeline = Some(Vp9Pipeline {
            _config: config,
            context,
            surfaces: surfaces.into_iter().map(Some).collect(),
            coded_width,
            coded_height,
            nv12_format,
        });
        Ok(())
    }

    /// Decode one picture given its already-parsed header and the raw packet bytes.
    #[allow(
        clippy::too_many_lines,
        reason = "linear per-picture decode sequence (reference-frames snapshot -> destination \
                  slot allocate -> parameter buffers -> GPU submit -> output -> ref_table \
                  update) — mirrors this crate's H.264/AV1 decode_one's identical allow/reasoning"
    )]
    fn decode_picture(
        &mut self,
        packet: &Packet,
        header: &Header,
    ) -> Result<VideoFrame, DecodeError> {
        self.ensure_pipeline(header.width, header.height)?;

        // Snapshot every occupied logical slot's real VASurfaceID before taking the decode
        // target's physical surface out of the pool — `free_pool_index()` never returns an
        // index any ref_table entry currently references (see that method's own doc comment),
        // so this ordering is not load-bearing, but computed first for clarity regardless.
        let mut reference_frames = [VA_INVALID_ID; VP9_REF_SLOTS];
        {
            let pipeline = self.pipeline.as_ref().ok_or(DecodeError::Backend)?;
            for (i, slot) in reference_frames.iter_mut().enumerate() {
                if let Some(entry) = self.ref_table.get(i)
                    && let Some(surface) = pipeline
                        .surfaces
                        .get(entry.pool_index)
                        .and_then(Option::as_ref)
                {
                    *slot = surface.id();
                }
            }
        }

        let target_index = self.ref_table.free_pool_index();
        let pipeline = self.pipeline.as_mut().ok_or(DecodeError::Backend)?;
        let surface = pipeline
            .surfaces
            .get_mut(target_index)
            .and_then(Option::take)
            .ok_or(DecodeError::Backend)?;
        let context = Rc::clone(&pipeline.context);
        let coded_width = pipeline.coded_width;
        let coded_height = pipeline.coded_height;
        let nv12_format = pipeline.nv12_format;
        // No `surface_id` local needed here — unlike H.264/AV1, `PictureParameterBufferVP9` has
        // no "current frame" field at all; VA-API infers the decode target from the `Picture`'s
        // own surface (`Picture::begin`'s internal `vaBeginPicture(context, render_target)`).

        let outcome = (|| -> Result<(Bytes, Surface<()>), DecodeError> {
            let pic_param = build_pic_param(header, reference_frames)?;
            let slice_param = build_slice_param(header, packet.payload.len())?;
            let slice_data_bytes = packet.payload.as_ref().to_vec();

            let pic_param_buf = context
                .create_buffer(BufferType::PictureParameter(PictureParameter::VP9(
                    pic_param,
                )))
                .map_err(|_| DecodeError::Backend)?;
            let slice_param_buf = context
                .create_buffer(BufferType::SliceParameter(SliceParameter::VP9(slice_param)))
                .map_err(|_| DecodeError::Backend)?;
            let slice_data_buf = context
                .create_buffer(BufferType::SliceData(slice_data_bytes))
                .map_err(|_| DecodeError::Backend)?;

            let timestamp = u64::try_from(packet.pts).unwrap_or(0);
            let mut picture = Picture::new(timestamp, Rc::clone(&context), surface);
            picture.add_buffer(pic_param_buf);
            picture.add_buffer(slice_param_buf);
            picture.add_buffer(slice_data_buf);

            let picture = picture.begin::<()>().map_err(|_| DecodeError::Backend)?;
            let picture = picture.render().map_err(|_| DecodeError::Backend)?;
            let picture = picture.end().map_err(|_| DecodeError::Backend)?;
            let picture = picture
                .sync::<()>()
                .map_err(|(_e, _pic)| DecodeError::Backend)?;

            let image = picture
                .create_image::<()>(
                    nv12_format,
                    (coded_width, coded_height),
                    (coded_width, coded_height),
                )
                .map_err(|_| DecodeError::Backend)?;
            let va_image = *image.image();
            let bytes = copy_nv12_from_planes(
                image.as_ref(),
                coded_width,
                coded_height,
                va_image.pitches[0],
                va_image.offsets[0],
                va_image.pitches[1],
                va_image.offsets[1],
            );
            drop(image);

            let surface = picture.take_surface().map_err(|_| DecodeError::Backend)?;
            Ok((bytes, surface))
        })();

        match outcome {
            Ok((data, returned_surface)) => {
                if let Some(pipeline) = self.pipeline.as_mut()
                    && let Some(slot) = pipeline.surfaces.get_mut(target_index)
                {
                    *slot = Some(returned_surface);
                }
                self.ref_table.refresh(
                    header.refresh_frame_flags,
                    target_index,
                    header.width,
                    header.height,
                );
                Ok(VideoFrame {
                    pts: packet.pts,
                    duration: packet.duration,
                    width: coded_width,
                    height: coded_height,
                    format: PixelFormat::Nv12,
                    storage: VideoFrameStorage::Cpu { data },
                })
            }
            Err(e) => {
                if let Some(pipeline) = self.pipeline.as_mut() {
                    let fresh = fresh_surface_or_placeholder(&context, coded_width, coded_height);
                    if let Some(slot) = pipeline.surfaces.get_mut(target_index) {
                        *slot = Some(fresh);
                    }
                }
                // Decode failed — this picture never becomes a valid reference; ref_table is
                // intentionally left untouched (target_index naturally stays "free" since no
                // entry ever pointed at it).
                Err(e)
            }
        }
    }
}

impl VideoDecoder for VaapiVp9Decoder {
    fn stream_info(&self) -> &StreamInfo {
        &self.info
    }

    fn push_packet(&mut self, packet: &Packet) -> Result<(), DecodeError> {
        if self.flushed {
            return Err(DecodeError::Closed);
        }
        if packet.is_discard {
            return Ok(());
        }
        // One Packet carries exactly one VP9 frame's bitstream — no superframe bundling, no
        // OBU-style multi-unit framing (unlike this crate's AV1 sibling) — see the ADR's own
        // § Scope.
        let header = Header::parse(packet.payload.as_ref(), &self.ref_table)?;
        let frame = self.decode_picture(packet, &header)?;
        self.pending.push_back(frame);
        Ok(())
    }

    fn poll_frame(&mut self) -> Result<Option<VideoFrame>, DecodeError> {
        Ok(self.pending.pop_front())
    }

    fn flush(&mut self) -> Result<(), DecodeError> {
        // Every push_packet already runs its decode synchronously (vaSyncSurface before
        // returning) — no pending driver pipeline to drain, matches this crate's H.264/AV1
        // flush.
        self.flushed = true;
        Ok(())
    }
}

/// Build `VADecPictureParameterBufferVP9`. `mb_segment_tree_probs`/`segment_pred_probs` are
/// always all-zero — never read by the driver when `segmentation_enabled == 0` (always the case
/// this crate's scope accepts), but passed as real, valid, all-zero arrays rather than skipped,
/// matching this crate's AV1 sibling's "never omit, always real" discipline (see the ADR's own
/// § VA-API-specific plumbing).
fn build_pic_param(
    header: &Header,
    reference_frames: [cros_libva::VASurfaceID; VP9_REF_SLOTS],
) -> Result<PictureParameterBufferVP9, DecodeError> {
    let width = u16::try_from(header.width).map_err(|_| DecodeError::InvalidInput)?;
    let height = u16::try_from(header.height).map_err(|_| DecodeError::InvalidInput)?;
    let frame_header_length_in_bytes =
        u8::try_from(header.frame_header_length_in_bytes).map_err(|_| DecodeError::InvalidInput)?;

    let pic_fields = VP9PicFields::new(
        1,                         // subsampling_x: 4:2:0 always (Profile 0 enforced)
        1,                         // subsampling_y
        u32::from(!header.is_key), // frame_type: 0 = KEY_FRAME, 1 = INTER_FRAME
        1,                         // show_frame: always 1, enforced by header::parse
        u32::from(header.error_resilient_mode),
        0, // intra_only: never reached this scope (show_frame forced 1)
        u32::from(header.allow_high_precision_mv),
        u32::from(header.interpolation_filter),
        u32::from(header.frame_parallel_decoding_mode),
        u32::from(header.reset_frame_context),
        u32::from(header.refresh_frame_context),
        u32::from(header.frame_context_idx),
        0, // segmentation_enabled: always rejected upstream when signaled
        0, // segmentation_temporal_update
        0, // segmentation_update_map
        u32::from(header.ref_frame_idx[0]), // last_ref_frame
        u32::from(header.ref_frame_sign_bias[0]),
        u32::from(header.ref_frame_idx[1]), // golden_ref_frame
        u32::from(header.ref_frame_sign_bias[1]),
        u32::from(header.ref_frame_idx[2]), // alt_ref_frame
        u32::from(header.ref_frame_sign_bias[2]),
        0, // lossless_flag: rejected upstream when signaled
    );

    Ok(PictureParameterBufferVP9::new(
        width,
        height,
        reference_frames,
        &pic_fields,
        header.loop_filter.level,
        header.loop_filter.sharpness,
        0, // log2_tile_rows: single tile enforced by tile_info::parse
        0, // log2_tile_columns
        frame_header_length_in_bytes,
        header.first_partition_size,
        [0u8; 7], // mb_segment_tree_probs: see this function's own doc comment
        [0u8; 3], // segment_pred_probs
        0,        // profile: Profile 0 enforced
        8,        // bit_depth: 8-bit only
    ))
}

/// Build `VASliceParameterBufferVP9` for this crate's single-tile scope. The `SliceData` buffer
/// carries the **whole** packet payload (not pre-sliced past the uncompressed header) —
/// `slice_data_offset` points past `frame_header_length_in_bytes` within that same buffer, and
/// `slice_data_size` covers the remaining bytes: the driver independently re-parses
/// `uncompressed_header()` from the leading bytes of that same buffer (see
/// `loop_filter`'s module doc comment for why this crate's own parsed loop-filter deltas need
/// not be forwarded structurally), using `frame_header_length_in_bytes`/`first_partition_size`
/// to locate the compressed header and tile data. Matches `FFmpeg`'s own real
/// `vaapi_vp9.c` convention (`frame_header_length_in_bytes = h->h.uncompressed_header_size`;
/// `first_partition_size = h->h.compressed_header_size`), confirmed this session.
fn build_slice_param(
    header: &Header,
    total_len: usize,
) -> Result<SliceParameterBufferVP9, DecodeError> {
    let slice_data_offset = u32::try_from(header.frame_header_length_in_bytes)
        .map_err(|_| DecodeError::InvalidInput)?;
    let remaining = total_len
        .checked_sub(header.frame_header_length_in_bytes)
        .ok_or(DecodeError::InvalidInput)?;
    let slice_data_size = u32::try_from(remaining).map_err(|_| DecodeError::InvalidInput)?;

    // Mandatory, non-`Option` per-segment table — always real, always all-disabled (matches
    // this crate's AV1 sibling's identical `AV1Segmentation`/`AV1FilmGrain` discipline).
    let seg_param: [SegmentParameterVP9; 8] = std::array::from_fn(|_| {
        let seg_flags = VP9SegmentFlags::new(0, 0, 0);
        SegmentParameterVP9::new(&seg_flags, [[0u8; 2]; 4], 0, 0, 0, 0)
    });

    Ok(SliceParameterBufferVP9::new(
        slice_data_size,
        slice_data_offset,
        0, // slice_data_flag = VA_SLICE_DATA_FLAG_ALL
        seg_param,
    ))
}

/// After a `Picture` consumes its surface, this crate cannot recover the exact same [`Surface`]
/// object on any of `decode_picture`'s error paths — mirrors this crate's H.264/AV1
/// `fresh_surface_or_placeholder` rationale exactly.
fn fresh_surface_or_placeholder(
    context: &Rc<Context>,
    coded_width: u32,
    coded_height: u32,
) -> Surface<()> {
    let display = context.display();
    display
        .create_surfaces(
            VA_RT_FORMAT_YUV420,
            Some(VA_FOURCC_NV12),
            coded_width,
            coded_height,
            None,
            vec![()],
        )
        .ok()
        .and_then(|mut v| v.pop())
        .unwrap_or_else(|| placeholder_surface(display, coded_width, coded_height))
}

/// Last-resort placeholder when even a minimal surface allocation fails — mirrors this crate's
/// H.264/AV1 `placeholder_surface` exactly (session is already unusable at that point).
fn placeholder_surface(display: &Rc<Display>, coded_width: u32, coded_height: u32) -> Surface<()> {
    #[allow(
        clippy::expect_used,
        reason = "session is already unrecoverable by this point; see caller doc comment"
    )]
    display
        .create_surfaces(
            VA_RT_FORMAT_YUV420,
            Some(VA_FOURCC_NV12),
            coded_width,
            coded_height,
            None,
            vec![()],
        )
        .expect("VA-API display known-good at session open time")
        .pop()
        .expect("exactly one surface requested")
}

fn validate(config: &VideoDecoderConfig) -> Result<(), DecodeError> {
    // This decoder only ever handles VP9 — an H.264/AV1 config must route to
    // VaapiH264Decoder/VaapiAv1Decoder instead, not silently be accepted here.
    if config.codec != mediaway_common::CodecKind::Vp9 {
        return Err(DecodeError::Unsupported);
    }
    if config.pixel_format != PixelFormat::Nv12 {
        return Err(DecodeError::Unsupported);
    }
    if config.time_base.den == 0 {
        return Err(DecodeError::InvalidInput);
    }
    Ok(())
}

const fn round_up_8(value: u32) -> u32 {
    value.div_ceil(8) * 8
}

#[allow(clippy::missing_const_for_fn, reason = "StreamInfo holds Bytes")]
fn stream_info_from(config: &VideoDecoderConfig) -> StreamInfo {
    StreamInfo::Video {
        id: 0,
        codec: config.codec,
        time_base: config.time_base,
        geometry: VideoGeometry {
            width: config.width,
            height: config.height,
        },
        extra_data: config.extra_data.clone(), // clone: owned StreamInfo snapshot at open
    }
}

#[cfg(test)]
#[path = "vp9_tests.rs"]
mod tests;
