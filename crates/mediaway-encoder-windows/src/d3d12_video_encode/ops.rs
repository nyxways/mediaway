//! Per-frame D3D12 recording: CPU→GPU NV12 upload+copy, `EncodeFrame` submission, and
//! compressed-bitstream readback. Split out of [`super`] to keep files under the
//! 1000-line source limit — see [`super::D3d12VideoEncoder`] for the owning struct.

use mediaway_common::{Bytes, Packet};
use mediaway_encoder::EncodeError;
use std::mem::ManuallyDrop;
use windows::Win32::Graphics::Direct3D12::{
    D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_COPY_DEST,
    D3D12_RESOURCE_STATE_VIDEO_ENCODE_READ, D3D12_RESOURCE_STATE_VIDEO_ENCODE_WRITE,
    ID3D12CommandList, ID3D12PipelineState,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_NV12, DXGI_FORMAT_R8_UNORM, DXGI_FORMAT_R8G8_UNORM, DXGI_RATIONAL,
};
use windows::Win32::Media::MediaFoundation::{
    D3D12_VIDEO_ENCODE_REFERENCE_FRAMES, D3D12_VIDEO_ENCODER_CODEC_H264,
    D3D12_VIDEO_ENCODER_COMPRESSED_BITSTREAM, D3D12_VIDEO_ENCODER_ENCODE_OPERATION_METADATA_BUFFER,
    D3D12_VIDEO_ENCODER_ENCODEFRAME_INPUT_ARGUMENTS,
    D3D12_VIDEO_ENCODER_ENCODEFRAME_OUTPUT_ARGUMENTS,
    D3D12_VIDEO_ENCODER_FRAME_SUBREGION_LAYOUT_MODE_FULL_FRAME,
    D3D12_VIDEO_ENCODER_FRAME_TYPE_H264_IDR_FRAME, D3D12_VIDEO_ENCODER_INTRA_REFRESH,
    D3D12_VIDEO_ENCODER_INTRA_REFRESH_MODE_NONE, D3D12_VIDEO_ENCODER_OUTPUT_METADATA,
    D3D12_VIDEO_ENCODER_PICTURE_CONTROL_CODEC_DATA,
    D3D12_VIDEO_ENCODER_PICTURE_CONTROL_CODEC_DATA_0,
    D3D12_VIDEO_ENCODER_PICTURE_CONTROL_CODEC_DATA_H264,
    D3D12_VIDEO_ENCODER_PICTURE_CONTROL_CODEC_DATA_H264_FLAG_NONE,
    D3D12_VIDEO_ENCODER_PICTURE_CONTROL_DESC, D3D12_VIDEO_ENCODER_PICTURE_CONTROL_FLAG_NONE,
    D3D12_VIDEO_ENCODER_PICTURE_CONTROL_SUBREGIONS_LAYOUT_DATA,
    D3D12_VIDEO_ENCODER_PICTURE_RESOLUTION_DESC, D3D12_VIDEO_ENCODER_PROFILE_DESC,
    D3D12_VIDEO_ENCODER_PROFILE_DESC_0, D3D12_VIDEO_ENCODER_PROFILE_H264,
    D3D12_VIDEO_ENCODER_PROFILE_H264_MAIN, D3D12_VIDEO_ENCODER_RATE_CONTROL,
    D3D12_VIDEO_ENCODER_RATE_CONTROL_CONFIGURATION_PARAMS,
    D3D12_VIDEO_ENCODER_RATE_CONTROL_CONFIGURATION_PARAMS_0, D3D12_VIDEO_ENCODER_RATE_CONTROL_CQP,
    D3D12_VIDEO_ENCODER_RATE_CONTROL_FLAG_NONE, D3D12_VIDEO_ENCODER_RATE_CONTROL_MODE_CQP,
    D3D12_VIDEO_ENCODER_RECONSTRUCTED_PICTURE,
    D3D12_VIDEO_ENCODER_RESOLVE_METADATA_INPUT_ARGUMENTS,
    D3D12_VIDEO_ENCODER_RESOLVE_METADATA_OUTPUT_ARGUMENTS,
    D3D12_VIDEO_ENCODER_SEQUENCE_CONTROL_DESC, D3D12_VIDEO_ENCODER_SEQUENCE_CONTROL_FLAG_NONE,
    D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE, D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_0,
    D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_H264,
};

use windows::core::Interface;

use super::D3d12VideoEncoder;
use super::util::{
    borrow_resource, data_size, placed_footprint_copy_location, signal_and_wait,
    subresource_copy_location, transition_barrier, write_nv12_upload,
};

impl D3d12VideoEncoder {
    pub(super) fn upload_and_copy(&mut self, nv12: &[u8]) -> Result<(), EncodeError> {
        write_nv12_upload(
            &self.upload_buffer,
            nv12,
            self.width,
            self.height,
            self.row_pitch,
            self.luma_size,
        )?;

        // SAFETY: freshly created/idle allocator+list (fully synchronous per frame).
        unsafe {
            self.copy_allocator
                .Reset()
                .map_err(|_| EncodeError::Backend)?;
            self.copy_list
                .Reset(&self.copy_allocator, None::<&ID3D12PipelineState>)
                .map_err(|_| EncodeError::Backend)?;
        }

        let to_copy_dest = transition_barrier(
            &self.input_texture,
            D3D12_RESOURCE_STATE_COMMON,
            D3D12_RESOURCE_STATE_COPY_DEST,
        );
        // SAFETY: recording on a freshly reset COPY-type command list.
        unsafe { self.copy_list.ResourceBarrier(&[to_copy_dest]) };

        let luma_dst = subresource_copy_location(&self.input_texture, 0);
        let luma_src = placed_footprint_copy_location(
            &self.upload_buffer,
            0,
            DXGI_FORMAT_R8_UNORM,
            self.width,
            self.height,
            self.row_pitch,
        );
        // SAFETY: `input_texture` is COPY_DEST, `upload_buffer` is GENERIC_READ.
        unsafe {
            self.copy_list.CopyTextureRegion(
                &raw const luma_dst,
                0,
                0,
                0,
                &raw const luma_src,
                None,
            );
        }

        let uv_dst = subresource_copy_location(&self.input_texture, 1);
        let uv_src = placed_footprint_copy_location(
            &self.upload_buffer,
            self.luma_size,
            DXGI_FORMAT_R8G8_UNORM,
            self.width / 2,
            self.height / 2,
            self.row_pitch,
        );
        // SAFETY: same resources/states as the luma copy above.
        unsafe {
            self.copy_list
                .CopyTextureRegion(&raw const uv_dst, 0, 0, 0, &raw const uv_src, None);
        }

        let to_common = transition_barrier(
            &self.input_texture,
            D3D12_RESOURCE_STATE_COPY_DEST,
            D3D12_RESOURCE_STATE_COMMON,
        );
        // SAFETY: recording on the same COPY-type command list.
        unsafe { self.copy_list.ResourceBarrier(&[to_common]) };

        // SAFETY: matched Reset/Close pair on this list.
        unsafe { self.copy_list.Close() }.map_err(|_| EncodeError::Backend)?;
        let generic: ID3D12CommandList = self.copy_list.cast().map_err(|_| EncodeError::Backend)?;
        // SAFETY: `generic` is the just-closed `copy_list`, valid for this COPY queue.
        unsafe { self.copy_queue.ExecuteCommandLists(&[Some(generic)]) };
        signal_and_wait(
            &self.copy_queue,
            &self.fence,
            self.fence_event,
            &mut self.fence_value,
        )
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one EncodeFrame call needs many populated D3D12 structs; splitting further fragments one atomic recording sequence"
    )]
    pub(super) fn encode_frame_h264(
        &mut self,
        pts: i64,
        duration: u64,
        mut gop: D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_H264,
    ) -> Result<Packet, EncodeError> {
        // SAFETY: freshly created/idle allocator+list (fully synchronous per frame).
        unsafe {
            self.encode_allocator
                .Reset()
                .map_err(|_| EncodeError::Backend)?;
            self.encode_list
                .Reset(&self.encode_allocator)
                .map_err(|_| EncodeError::Backend)?;
        }

        let barriers_before = [
            transition_barrier(
                &self.input_texture,
                D3D12_RESOURCE_STATE_COMMON,
                D3D12_RESOURCE_STATE_VIDEO_ENCODE_READ,
            ),
            transition_barrier(
                &self.bitstream_buffer,
                D3D12_RESOURCE_STATE_COMMON,
                D3D12_RESOURCE_STATE_VIDEO_ENCODE_WRITE,
            ),
            transition_barrier(
                &self.metadata_buffer,
                D3D12_RESOURCE_STATE_COMMON,
                D3D12_RESOURCE_STATE_VIDEO_ENCODE_WRITE,
            ),
            transition_barrier(
                &self.resolved_metadata_buffer,
                D3D12_RESOURCE_STATE_COMMON,
                D3D12_RESOURCE_STATE_VIDEO_ENCODE_WRITE,
            ),
        ];
        // SAFETY: `VIDEO_ENCODE_READ`/`WRITE` transitions are only valid recorded on a
        // video-encode command list — this is one.
        unsafe { self.encode_list.ResourceBarrier(&barriers_before) };

        let resolution = D3D12_VIDEO_ENCODER_PICTURE_RESOLUTION_DESC {
            Width: self.width,
            Height: self.height,
        };

        let gop_desc = D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE {
            DataSize: data_size::<D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_H264>(),
            Anonymous: D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_0 {
                pH264GroupOfPictures: &raw mut gop,
            },
        };

        let rc_cqp = self.rc_cqp;
        let rc = D3D12_VIDEO_ENCODER_RATE_CONTROL {
            Mode: D3D12_VIDEO_ENCODER_RATE_CONTROL_MODE_CQP,
            Flags: D3D12_VIDEO_ENCODER_RATE_CONTROL_FLAG_NONE,
            ConfigParams: D3D12_VIDEO_ENCODER_RATE_CONTROL_CONFIGURATION_PARAMS {
                DataSize: data_size::<D3D12_VIDEO_ENCODER_RATE_CONTROL_CQP>(),
                Anonymous: D3D12_VIDEO_ENCODER_RATE_CONTROL_CONFIGURATION_PARAMS_0 {
                    pConfiguration_CQP: &raw const rc_cqp,
                },
            },
            TargetFrameRate: DXGI_RATIONAL {
                Numerator: self.fps_num,
                Denominator: self.fps_den,
            },
        };

        let mut pic_data = D3D12_VIDEO_ENCODER_PICTURE_CONTROL_CODEC_DATA_H264 {
            Flags: D3D12_VIDEO_ENCODER_PICTURE_CONTROL_CODEC_DATA_H264_FLAG_NONE,
            FrameType: D3D12_VIDEO_ENCODER_FRAME_TYPE_H264_IDR_FRAME,
            pic_parameter_set_id: 0,
            idr_pic_id: self.frame_counter,
            PictureOrderCountNumber: 0,
            FrameDecodingOrderNumber: 0,
            TemporalLayerIndex: 0,
            List0ReferenceFramesCount: 0,
            pList0ReferenceFrames: std::ptr::null_mut(),
            List1ReferenceFramesCount: 0,
            pList1ReferenceFrames: std::ptr::null_mut(),
            ReferenceFramesReconPictureDescriptorsCount: 0,
            pReferenceFramesReconPictureDescriptors: std::ptr::null_mut(),
            adaptive_ref_pic_marking_mode_flag: 0,
            RefPicMarkingOperationsCommandsCount: 0,
            pRefPicMarkingOperationsCommands: std::ptr::null_mut(),
            List0RefPicModificationsCount: 0,
            pList0RefPicModifications: std::ptr::null_mut(),
            List1RefPicModificationsCount: 0,
            pList1RefPicModifications: std::ptr::null_mut(),
            QPMapValuesCount: 0,
            pRateControlQPMap: std::ptr::null_mut(),
        };
        self.frame_counter = self.frame_counter.wrapping_add(1);

        let pic_control_data = D3D12_VIDEO_ENCODER_PICTURE_CONTROL_CODEC_DATA {
            DataSize: data_size::<D3D12_VIDEO_ENCODER_PICTURE_CONTROL_CODEC_DATA_H264>(),
            Anonymous: D3D12_VIDEO_ENCODER_PICTURE_CONTROL_CODEC_DATA_0 {
                pH264PicData: &raw mut pic_data,
            },
        };

        let input_args = D3D12_VIDEO_ENCODER_ENCODEFRAME_INPUT_ARGUMENTS {
            SequenceControlDesc: D3D12_VIDEO_ENCODER_SEQUENCE_CONTROL_DESC {
                Flags: D3D12_VIDEO_ENCODER_SEQUENCE_CONTROL_FLAG_NONE,
                IntraRefreshConfig: D3D12_VIDEO_ENCODER_INTRA_REFRESH {
                    Mode: D3D12_VIDEO_ENCODER_INTRA_REFRESH_MODE_NONE,
                    IntraRefreshDuration: 0,
                },
                RateControl: rc,
                PictureTargetResolution: resolution,
                SelectedLayoutMode: D3D12_VIDEO_ENCODER_FRAME_SUBREGION_LAYOUT_MODE_FULL_FRAME,
                FrameSubregionsLayoutData:
                    D3D12_VIDEO_ENCODER_PICTURE_CONTROL_SUBREGIONS_LAYOUT_DATA::default(),
                CodecGopSequence: gop_desc,
            },
            PictureControlDesc: D3D12_VIDEO_ENCODER_PICTURE_CONTROL_DESC {
                IntraRefreshFrameIndex: 0,
                Flags: D3D12_VIDEO_ENCODER_PICTURE_CONTROL_FLAG_NONE,
                PictureControlCodecData: pic_control_data,
                ReferenceFrames: D3D12_VIDEO_ENCODE_REFERENCE_FRAMES {
                    NumTexture2Ds: 0,
                    ppTexture2Ds: std::ptr::null_mut(),
                    pSubresources: std::ptr::null_mut(),
                },
            },
            pInputFrame: borrow_resource(&self.input_texture),
            InputFrameSubresource: 0,
            CurrentFrameBitstreamMetadataSize: u32::try_from(self.header_len_aligned)
                .unwrap_or(u32::MAX),
        };

        let output_args = D3D12_VIDEO_ENCODER_ENCODEFRAME_OUTPUT_ARGUMENTS {
            Bitstream: D3D12_VIDEO_ENCODER_COMPRESSED_BITSTREAM {
                pBuffer: borrow_resource(&self.bitstream_buffer),
                FrameStartOffset: self.header_len_aligned,
            },
            ReconstructedPicture: D3D12_VIDEO_ENCODER_RECONSTRUCTED_PICTURE {
                pReconstructedPicture: ManuallyDrop::new(None),
                ReconstructedPictureSubresource: 0,
            },
            EncoderOutputMetadata: D3D12_VIDEO_ENCODER_ENCODE_OPERATION_METADATA_BUFFER {
                pBuffer: borrow_resource(&self.metadata_buffer),
                Offset: 0,
            },
        };

        // SAFETY: encoder/heap were created for this exact codec/profile/resolution;
        // buffers were sized from `D3D12_FEATURE_VIDEO_ENCODER_RESOURCE_REQUIREMENTS` at
        // `open`. No reference frames — `ReconstructedPicture` is `None` per the API's
        // documented "only needed if used as a reference" contract.
        unsafe {
            self.encode_list.EncodeFrame(
                &self.encoder,
                &self.encoder_heap,
                &raw const input_args,
                &raw const output_args,
            );
        }

        let metadata_write_to_read = transition_barrier(
            &self.metadata_buffer,
            D3D12_RESOURCE_STATE_VIDEO_ENCODE_WRITE,
            D3D12_RESOURCE_STATE_VIDEO_ENCODE_READ,
        );
        // SAFETY: same video-encode command list as the `EncodeFrame` call above.
        unsafe { self.encode_list.ResourceBarrier(&[metadata_write_to_read]) };

        let mut profile_h264: D3D12_VIDEO_ENCODER_PROFILE_H264 =
            D3D12_VIDEO_ENCODER_PROFILE_H264_MAIN;
        let profile_desc = D3D12_VIDEO_ENCODER_PROFILE_DESC {
            DataSize: data_size::<D3D12_VIDEO_ENCODER_PROFILE_H264>(),
            Anonymous: D3D12_VIDEO_ENCODER_PROFILE_DESC_0 {
                pH264Profile: &raw mut profile_h264,
            },
        };
        let input_metadata = D3D12_VIDEO_ENCODER_RESOLVE_METADATA_INPUT_ARGUMENTS {
            EncoderCodec: D3D12_VIDEO_ENCODER_CODEC_H264,
            EncoderProfile: profile_desc,
            EncoderInputFormat: DXGI_FORMAT_NV12,
            EncodedPictureEffectiveResolution: resolution,
            HWLayoutMetadata: D3D12_VIDEO_ENCODER_ENCODE_OPERATION_METADATA_BUFFER {
                pBuffer: borrow_resource(&self.metadata_buffer),
                Offset: 0,
            },
        };
        let output_metadata = D3D12_VIDEO_ENCODER_RESOLVE_METADATA_OUTPUT_ARGUMENTS {
            ResolvedLayoutMetadata: D3D12_VIDEO_ENCODER_ENCODE_OPERATION_METADATA_BUFFER {
                pBuffer: borrow_resource(&self.resolved_metadata_buffer),
                Offset: 0,
            },
        };
        // SAFETY: same video-encode command list; `HWLayoutMetadata` was just transitioned
        // to `VIDEO_ENCODE_READ` above.
        unsafe {
            self.encode_list.ResolveEncoderOutputMetadata(
                &raw const input_metadata,
                &raw const output_metadata,
            );
        }

        let barriers_after = [
            transition_barrier(
                &self.input_texture,
                D3D12_RESOURCE_STATE_VIDEO_ENCODE_READ,
                D3D12_RESOURCE_STATE_COMMON,
            ),
            transition_barrier(
                &self.bitstream_buffer,
                D3D12_RESOURCE_STATE_VIDEO_ENCODE_WRITE,
                D3D12_RESOURCE_STATE_COMMON,
            ),
            transition_barrier(
                &self.metadata_buffer,
                D3D12_RESOURCE_STATE_VIDEO_ENCODE_READ,
                D3D12_RESOURCE_STATE_COMMON,
            ),
            transition_barrier(
                &self.resolved_metadata_buffer,
                D3D12_RESOURCE_STATE_VIDEO_ENCODE_WRITE,
                D3D12_RESOURCE_STATE_COMMON,
            ),
        ];
        // SAFETY: same video-encode command list as every barrier/call above.
        unsafe { self.encode_list.ResourceBarrier(&barriers_after) };

        // SAFETY: matched Reset/Close pair on this list.
        unsafe { self.encode_list.Close() }.map_err(|_| EncodeError::Backend)?;
        let generic: ID3D12CommandList =
            self.encode_list.cast().map_err(|_| EncodeError::Backend)?;
        // SAFETY: `generic` is the just-closed `encode_list`, valid for this VIDEO_ENCODE queue.
        unsafe { self.encode_queue.ExecuteCommandLists(&[Some(generic)]) };
        signal_and_wait(
            &self.encode_queue,
            &self.fence,
            self.fence_event,
            &mut self.fence_value,
        )?;

        self.read_packet(pts, duration)
    }

    pub(super) fn read_packet(&mut self, pts: i64, duration: u64) -> Result<Packet, EncodeError> {
        let mut resolved_ptr: *mut u8 = std::ptr::null_mut();
        // SAFETY: READBACK-heap resource; CPU `Map` is valid regardless of the GPU-side
        // `D3D12_RESOURCE_STATES` the last-recorded command list left it in.
        unsafe {
            self.resolved_metadata_buffer
                .Map(0, None, Some(std::ptr::from_mut(&mut resolved_ptr).cast()))
                .map_err(|_| EncodeError::Backend)?;
        }
        if resolved_ptr.is_null() {
            return Err(EncodeError::Backend);
        }
        // SAFETY: buffer was sized >= `size_of::<D3D12_VIDEO_ENCODER_OUTPUT_METADATA>()` at
        // `open`; the driver writes a full struct at offset 0 during `ResolveEncoderOutputMetadata`.
        let meta = unsafe {
            resolved_ptr
                .cast::<D3D12_VIDEO_ENCODER_OUTPUT_METADATA>()
                .read_unaligned()
        };
        // SAFETY: matches the `Map` above.
        unsafe { self.resolved_metadata_buffer.Unmap(0, None) };

        // `D3D12_VIDEO_ENCODER_ENCODE_ERROR_FLAG_NO_ERROR` is `0` — any nonzero flag is an error.
        if meta.EncodeErrorFlags != 0 {
            return Err(EncodeError::Backend);
        }
        let written = meta.EncodedBitstreamWrittenBytesCount;
        if written == 0 {
            return Err(EncodeError::Backend);
        }
        let written_usize = usize::try_from(written).map_err(|_| EncodeError::Backend)?;
        let offset_usize =
            usize::try_from(self.header_len_aligned).map_err(|_| EncodeError::Backend)?;
        let end = offset_usize
            .checked_add(written_usize)
            .ok_or(EncodeError::Backend)?;
        if end as u64 > self.bitstream_capacity {
            return Err(EncodeError::Backend);
        }

        let mut payload = self.header_bytes.clone(); // clone: own Packet payload built from the persistent per-session SPS/PPS bytes
        let mut slice_ptr: *mut u8 = std::ptr::null_mut();
        // SAFETY: READBACK-heap resource, same reasoning as the metadata `Map` above.
        unsafe {
            self.bitstream_buffer
                .Map(0, None, Some(std::ptr::from_mut(&mut slice_ptr).cast()))
                .map_err(|_| EncodeError::Backend)?;
        }
        if slice_ptr.is_null() {
            // SAFETY: matches the `Map` immediately above.
            unsafe { self.bitstream_buffer.Unmap(0, None) };
            return Err(EncodeError::Backend);
        }
        // SAFETY: `offset_usize + written_usize <= bitstream_capacity`, checked above; the
        // buffer was committed with that capacity at `open`.
        unsafe {
            let slice = std::slice::from_raw_parts(slice_ptr.add(offset_usize), written_usize);
            payload.extend_from_slice(slice);
        }
        // SAFETY: matches the `Map` above.
        unsafe { self.bitstream_buffer.Unmap(0, None) };

        Ok(Packet {
            stream_id: self.info.id(),
            pts,
            dts: pts,
            duration,
            is_keyframe: true,
            is_discard: false,
            payload: Bytes::from(payload),
        })
    }
}
