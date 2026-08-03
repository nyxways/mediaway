//! Per-frame D3D12 recording for HEVC — mirrors [`super::ops`]'s H.264 `encode_frame`, but
//! building the HEVC-flavored `D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE`/
//! `_PICTURE_CONTROL_CODEC_DATA`/`_PROFILE_DESC` union payloads. `upload_and_copy` (NV12
//! CPU→GPU staging) and `read_packet` (bitstream readback) are codec-agnostic and stay
//! shared in `ops.rs` — only the `EncodeFrame`/`ResolveEncoderOutputMetadata` argument
//! construction differs per codec, for the C-union reasons [`super::hevc`]'s doc comment
//! explains.

use crate::EncodeError;
use mediaway_common::Packet;
use std::mem::ManuallyDrop;
use windows::Win32::Graphics::Direct3D12::{
    D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_VIDEO_ENCODE_READ,
    D3D12_RESOURCE_STATE_VIDEO_ENCODE_WRITE, ID3D12CommandList,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_RATIONAL};
use windows::Win32::Media::MediaFoundation::{
    D3D12_VIDEO_ENCODE_REFERENCE_FRAMES, D3D12_VIDEO_ENCODER_CODEC_HEVC,
    D3D12_VIDEO_ENCODER_COMPRESSED_BITSTREAM, D3D12_VIDEO_ENCODER_ENCODE_OPERATION_METADATA_BUFFER,
    D3D12_VIDEO_ENCODER_ENCODEFRAME_INPUT_ARGUMENTS,
    D3D12_VIDEO_ENCODER_ENCODEFRAME_OUTPUT_ARGUMENTS,
    D3D12_VIDEO_ENCODER_FRAME_SUBREGION_LAYOUT_MODE_FULL_FRAME,
    D3D12_VIDEO_ENCODER_FRAME_TYPE_HEVC_IDR_FRAME, D3D12_VIDEO_ENCODER_INTRA_REFRESH,
    D3D12_VIDEO_ENCODER_INTRA_REFRESH_MODE_NONE, D3D12_VIDEO_ENCODER_PICTURE_CONTROL_CODEC_DATA,
    D3D12_VIDEO_ENCODER_PICTURE_CONTROL_CODEC_DATA_0,
    D3D12_VIDEO_ENCODER_PICTURE_CONTROL_CODEC_DATA_HEVC,
    D3D12_VIDEO_ENCODER_PICTURE_CONTROL_CODEC_DATA_HEVC_FLAG_NONE,
    D3D12_VIDEO_ENCODER_PICTURE_CONTROL_DESC, D3D12_VIDEO_ENCODER_PICTURE_CONTROL_FLAG_NONE,
    D3D12_VIDEO_ENCODER_PICTURE_CONTROL_SUBREGIONS_LAYOUT_DATA,
    D3D12_VIDEO_ENCODER_PICTURE_RESOLUTION_DESC, D3D12_VIDEO_ENCODER_PROFILE_DESC,
    D3D12_VIDEO_ENCODER_PROFILE_DESC_0, D3D12_VIDEO_ENCODER_PROFILE_HEVC,
    D3D12_VIDEO_ENCODER_PROFILE_HEVC_MAIN, D3D12_VIDEO_ENCODER_RATE_CONTROL,
    D3D12_VIDEO_ENCODER_RATE_CONTROL_CONFIGURATION_PARAMS,
    D3D12_VIDEO_ENCODER_RATE_CONTROL_CONFIGURATION_PARAMS_0, D3D12_VIDEO_ENCODER_RATE_CONTROL_CQP,
    D3D12_VIDEO_ENCODER_RATE_CONTROL_FLAG_NONE, D3D12_VIDEO_ENCODER_RATE_CONTROL_MODE_CQP,
    D3D12_VIDEO_ENCODER_RECONSTRUCTED_PICTURE,
    D3D12_VIDEO_ENCODER_RESOLVE_METADATA_INPUT_ARGUMENTS,
    D3D12_VIDEO_ENCODER_RESOLVE_METADATA_OUTPUT_ARGUMENTS,
    D3D12_VIDEO_ENCODER_SEQUENCE_CONTROL_DESC, D3D12_VIDEO_ENCODER_SEQUENCE_CONTROL_FLAG_NONE,
    D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE, D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_0,
    D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_HEVC,
};
use windows::core::Interface;

use super::D3d12VideoEncoder;
use super::util::{borrow_resource, data_size, signal_and_wait, transition_barrier};

impl D3d12VideoEncoder {
    #[allow(
        clippy::too_many_lines,
        reason = "one EncodeFrame call needs many populated D3D12 structs; mirrors ops::encode_frame's H.264 sibling"
    )]
    pub(super) fn encode_frame_hevc(
        &mut self,
        pts: i64,
        duration: u64,
        mut gop: D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_HEVC,
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
            DataSize: data_size::<D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_HEVC>(),
            Anonymous: D3D12_VIDEO_ENCODER_SEQUENCE_GOP_STRUCTURE_0 {
                pHEVCGroupOfPictures: &raw mut gop,
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

        let mut pic_data = D3D12_VIDEO_ENCODER_PICTURE_CONTROL_CODEC_DATA_HEVC {
            Flags: D3D12_VIDEO_ENCODER_PICTURE_CONTROL_CODEC_DATA_HEVC_FLAG_NONE,
            FrameType: D3D12_VIDEO_ENCODER_FRAME_TYPE_HEVC_IDR_FRAME,
            slice_pic_parameter_set_id: 0,
            PictureOrderCountNumber: 0,
            TemporalLayerIndex: 0,
            List0ReferenceFramesCount: 0,
            pList0ReferenceFrames: std::ptr::null_mut(),
            List1ReferenceFramesCount: 0,
            pList1ReferenceFrames: std::ptr::null_mut(),
            ReferenceFramesReconPictureDescriptorsCount: 0,
            pReferenceFramesReconPictureDescriptors: std::ptr::null_mut(),
            List0RefPicModificationsCount: 0,
            pList0RefPicModifications: std::ptr::null_mut(),
            List1RefPicModificationsCount: 0,
            pList1RefPicModifications: std::ptr::null_mut(),
            QPMapValuesCount: 0,
            pRateControlQPMap: std::ptr::null_mut(),
        };
        self.frame_counter = self.frame_counter.wrapping_add(1);

        let pic_control_data = D3D12_VIDEO_ENCODER_PICTURE_CONTROL_CODEC_DATA {
            DataSize: data_size::<D3D12_VIDEO_ENCODER_PICTURE_CONTROL_CODEC_DATA_HEVC>(),
            Anonymous: D3D12_VIDEO_ENCODER_PICTURE_CONTROL_CODEC_DATA_0 {
                pHEVCPicData: &raw mut pic_data,
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

        let mut profile_hevc: D3D12_VIDEO_ENCODER_PROFILE_HEVC =
            D3D12_VIDEO_ENCODER_PROFILE_HEVC_MAIN;
        let profile_desc = D3D12_VIDEO_ENCODER_PROFILE_DESC {
            DataSize: data_size::<D3D12_VIDEO_ENCODER_PROFILE_HEVC>(),
            Anonymous: D3D12_VIDEO_ENCODER_PROFILE_DESC_0 {
                pHEVCProfile: &raw mut profile_hevc,
            },
        };
        let input_metadata = D3D12_VIDEO_ENCODER_RESOLVE_METADATA_INPUT_ARGUMENTS {
            EncoderCodec: D3D12_VIDEO_ENCODER_CODEC_HEVC,
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
}
