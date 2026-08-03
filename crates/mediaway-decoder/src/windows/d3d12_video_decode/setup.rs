//! `open`-time D3D12 object creation: device/queue/allocator/list objects, the
//! `D3D12_FEATURE_VIDEO_DECODE_SUPPORT` capability query, `ID3D12VideoDecoder`/
//! `ID3D12VideoDecoderHeap` creation, and DPB texture-array allocation. Codec-generic —
//! every function takes a profile GUID / DXGI format rather than hardcoding H.264, so
//! a future HEVC/AV1 pass can call the same helpers (per ADR-0002's file-layout plan).

use crate::DecodeError;
use mediaway_common::NativeHandle;
use windows::Win32::Graphics::Direct3D12::{
    D3D12_COMMAND_LIST_FLAG_NONE, D3D12_COMMAND_LIST_TYPE, D3D12_COMMAND_QUEUE_DESC,
    D3D12_COMMAND_QUEUE_FLAG_NONE, D3D12_CPU_PAGE_PROPERTY_UNKNOWN, D3D12_HEAP_FLAG_NONE,
    D3D12_HEAP_PROPERTIES, D3D12_HEAP_TYPE, D3D12_HEAP_TYPE_DEFAULT, D3D12_MEMORY_POOL_UNKNOWN,
    D3D12_RESOURCE_DESC, D3D12_RESOURCE_DIMENSION_BUFFER, D3D12_RESOURCE_DIMENSION_TEXTURE2D,
    D3D12_RESOURCE_FLAG_NONE, D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATES,
    D3D12_TEXTURE_LAYOUT_ROW_MAJOR, D3D12_TEXTURE_LAYOUT_UNKNOWN, ID3D12CommandAllocator,
    ID3D12CommandQueue, ID3D12Device, ID3D12Device4, ID3D12Resource,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_NV12, DXGI_FORMAT_UNKNOWN, DXGI_RATIONAL, DXGI_SAMPLE_DESC,
};
use windows::Win32::Media::MediaFoundation::{
    D3D12_BITSTREAM_ENCRYPTION_TYPE_NONE, D3D12_FEATURE_DATA_VIDEO_DECODE_SUPPORT,
    D3D12_FEATURE_VIDEO_DECODE_SUPPORT, D3D12_VIDEO_DECODE_CONFIGURATION,
    D3D12_VIDEO_DECODE_CONFIGURATION_FLAGS, D3D12_VIDEO_DECODE_SUPPORT_FLAG_SUPPORTED,
    D3D12_VIDEO_DECODE_SUPPORT_FLAGS, D3D12_VIDEO_DECODE_TIER_NOT_SUPPORTED,
    D3D12_VIDEO_DECODER_DESC, D3D12_VIDEO_DECODER_HEAP_DESC,
    D3D12_VIDEO_FRAME_CODED_INTERLACE_TYPE_NONE, ID3D12VideoDecoder, ID3D12VideoDecoderHeap,
    ID3D12VideoDevice,
};
use windows::core::{GUID, Interface};

use super::util::data_size;

/// Borrow `device_handle` as an owned `ID3D12Device` (COM `AddRef`d for this session) —
/// same pattern `mediaway-encoder-windows`'s `d3d12_video_encode/setup.rs::device_from_handle`
/// uses.
pub(super) fn device_from_handle(device_handle: NativeHandle) -> Result<ID3D12Device, DecodeError> {
    let raw = device_handle.get() as *mut core::ffi::c_void;
    // SAFETY: caller guarantees a live `ID3D12Device*` for the session (config contract).
    let borrowed =
        unsafe { ID3D12Device::from_raw_borrowed(&raw) }.ok_or(DecodeError::InvalidInput)?;
    // clone: COM AddRef so we own the device for the session's lifetime
    Ok(borrowed.clone())
}

/// Query `D3D12_FEATURE_VIDEO_DECODE_SUPPORT` for `profile` at `width`x`height`,
/// `format`. Returns the driver's populated struct (callers read `DecodeTier` /
/// `ConfigurationFlags` for e.g. the height-alignment-32 requirement).
///
/// # Errors
///
/// [`DecodeError::Unsupported`] when the driver does not support this configuration at
/// all (`SupportFlags` lacks `SUPPORTED`).
pub(super) fn check_decode_support(
    video_device: &ID3D12VideoDevice,
    profile: GUID,
    width: u32,
    height: u32,
    format: DXGI_FORMAT,
) -> Result<D3D12_FEATURE_DATA_VIDEO_DECODE_SUPPORT, DecodeError> {
    let mut support = D3D12_FEATURE_DATA_VIDEO_DECODE_SUPPORT {
        NodeIndex: 0,
        Configuration: D3D12_VIDEO_DECODE_CONFIGURATION {
            DecodeProfile: profile,
            BitstreamEncryption: D3D12_BITSTREAM_ENCRYPTION_TYPE_NONE,
            InterlaceType: D3D12_VIDEO_FRAME_CODED_INTERLACE_TYPE_NONE,
        },
        Width: width,
        Height: height,
        DecodeFormat: format,
        FrameRate: DXGI_RATIONAL {
            Numerator: 30,
            Denominator: 1,
        },
        BitRate: 0,
        SupportFlags: D3D12_VIDEO_DECODE_SUPPORT_FLAGS::default(),
        ConfigurationFlags: D3D12_VIDEO_DECODE_CONFIGURATION_FLAGS::default(),
        DecodeTier: D3D12_VIDEO_DECODE_TIER_NOT_SUPPORTED,
    };
    // SAFETY: `support` is sized/typed exactly as `D3D12_FEATURE_DATA_VIDEO_DECODE_SUPPORT` expects.
    unsafe {
        video_device
            .CheckFeatureSupport(
                D3D12_FEATURE_VIDEO_DECODE_SUPPORT,
                std::ptr::from_mut(&mut support).cast(),
                data_size::<D3D12_FEATURE_DATA_VIDEO_DECODE_SUPPORT>(),
            )
            .map_err(|_err| DecodeError::Unsupported)?;
    }
    if !support
        .SupportFlags
        .contains(D3D12_VIDEO_DECODE_SUPPORT_FLAG_SUPPORTED)
    {
        return Err(DecodeError::Unsupported);
    }
    Ok(support)
}

/// Create `ID3D12VideoDecoder` + `ID3D12VideoDecoderHeap` for `profile` at
/// `width`x`height`, `format`, sized for `max_dpb_slots` decode-picture-buffer entries.
pub(super) fn create_decoder(
    video_device: &ID3D12VideoDevice,
    profile: GUID,
    width: u32,
    height: u32,
    format: DXGI_FORMAT,
    max_dpb_slots: u32,
) -> Result<(ID3D12VideoDecoder, ID3D12VideoDecoderHeap), DecodeError> {
    let configuration = D3D12_VIDEO_DECODE_CONFIGURATION {
        DecodeProfile: profile,
        BitstreamEncryption: D3D12_BITSTREAM_ENCRYPTION_TYPE_NONE,
        InterlaceType: D3D12_VIDEO_FRAME_CODED_INTERLACE_TYPE_NONE,
    };
    let decoder_desc = D3D12_VIDEO_DECODER_DESC {
        NodeMask: 0,
        Configuration: configuration,
    };
    // SAFETY: `decoder_desc` is a plain POD desc (no borrowed pointers).
    let decoder: ID3D12VideoDecoder =
        unsafe { video_device.CreateVideoDecoder(&raw const decoder_desc) }
            .map_err(|_err| DecodeError::Backend)?;

    let heap_desc = D3D12_VIDEO_DECODER_HEAP_DESC {
        NodeMask: 0,
        Configuration: configuration,
        DecodeWidth: width,
        DecodeHeight: height,
        Format: format,
        FrameRate: DXGI_RATIONAL {
            Numerator: 30,
            Denominator: 1,
        },
        BitRate: 0,
        MaxDecodePictureBufferCount: max_dpb_slots,
    };
    // SAFETY: `heap_desc` is a plain POD desc (no borrowed pointers).
    let decoder_heap: ID3D12VideoDecoderHeap =
        unsafe { video_device.CreateVideoDecoderHeap(&raw const heap_desc) }
            .map_err(|_err| DecodeError::Backend)?;

    Ok((decoder, decoder_heap))
}

pub(super) fn create_command_objects<T: Interface>(
    device: &ID3D12Device,
    device4: &ID3D12Device4,
    list_type: D3D12_COMMAND_LIST_TYPE,
) -> Result<(ID3D12CommandQueue, ID3D12CommandAllocator, T), DecodeError> {
    let queue_desc = D3D12_COMMAND_QUEUE_DESC {
        Type: list_type,
        Priority: 0,
        Flags: D3D12_COMMAND_QUEUE_FLAG_NONE,
        NodeMask: 0,
    };
    // SAFETY: plain POD desc, no borrowed pointers.
    let queue: ID3D12CommandQueue = unsafe { device.CreateCommandQueue(&raw const queue_desc) }
        .map_err(|_err| DecodeError::Backend)?;
    // SAFETY: `list_type` is a valid `D3D12_COMMAND_LIST_TYPE`.
    let allocator: ID3D12CommandAllocator =
        unsafe { device.CreateCommandAllocator(list_type) }.map_err(|_err| DecodeError::Backend)?;
    // SAFETY: `device4` was obtained via `cast` from the same live device.
    let list: T = unsafe { device4.CreateCommandList1(0, list_type, D3D12_COMMAND_LIST_FLAG_NONE) }
        .map_err(|_err| DecodeError::Backend)?;
    Ok((queue, allocator, list))
}

/// Allocate the fixed-size DPB texture array (ADR-0002's "texture array" DPB mode):
/// one `NV12` `ID3D12Resource` with `DepthOrArraySize == num_slots`, each array slice a
/// DPB slot addressed by subresource index.
pub(super) fn create_dpb_texture_array(
    device: &ID3D12Device,
    width: u32,
    height: u32,
    num_slots: u32,
) -> Result<ID3D12Resource, DecodeError> {
    let heap_props = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_DEFAULT,
        CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
        MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
        CreationNodeMask: 1,
        VisibleNodeMask: 1,
    };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Alignment: 0,
        Width: u64::from(width),
        Height: height,
        DepthOrArraySize: u16::try_from(num_slots).unwrap_or(u16::MAX),
        MipLevels: 1,
        Format: DXGI_FORMAT_NV12,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
        // Plain `NONE` — real-hardware finding (this session): `ALLOW_SIMULTANEOUS_
        // ACCESS` was tried first (reasoning: DPB slots are read/written across the
        // decode queue and whatever queue a caller samples a Zero-Copy handle from) but
        // is a real suspect in a `DXGI_ERROR_DEVICE_HUNG` TDR hit while debugging
        // `DecodeFrame1` on real hardware — it changes cache-coherency/tiling behavior
        // in ways FFmpeg's own decode-surface pools (`hwcontext_d3d12va.c`) do not
        // opt into by default. Reverted to the conservative default pending further
        // isolation; see ADR-0002 Addendum.
        Flags: D3D12_RESOURCE_FLAG_NONE,
    };
    let mut resource: Option<ID3D12Resource> = None;
    // SAFETY: `CreateCommittedResource` with a DEFAULT-heap NV12 texture array; caller-owned.
    unsafe {
        device
            .CreateCommittedResource(
                &raw const heap_props,
                D3D12_HEAP_FLAG_NONE,
                &raw const desc,
                D3D12_RESOURCE_STATE_COMMON,
                None,
                &raw mut resource,
            )
            .map_err(|_err| DecodeError::Backend)?;
    }
    resource.ok_or(DecodeError::Backend)
}

/// Create a linear buffer on the `D3D12_HEAP_TYPE_CUSTOM` equivalent of `heap_type`
/// (via `GetCustomHeapProperties`) at `initial_state`.
///
/// Mirrors `mediaway-encoder-windows`'s `d3d12_video_encode/setup.rs::create_linear_buffer`
/// exactly (same discovered workaround): resources on the *abstract* `UPLOAD`/`READBACK`
/// heap-type enum are restricted by the D3D12 debug layer to a narrow set of states
/// unrelated to `VIDEO_DECODE_READ`/`WRITE`; resolving to the concrete `CUSTOM` heap
/// properties upfront (same physical memory, same CPU `Map`/`Unmap` behavior) sidesteps
/// that abstract-type check, letting this module create the compressed-bitstream input
/// buffer directly in `D3D12_RESOURCE_STATE_VIDEO_DECODE_READ` with no barrier ever
/// needed (CPU `Map` calls are unaffected by GPU-side resource state).
pub(super) fn create_linear_buffer(
    device: &ID3D12Device,
    heap_type: D3D12_HEAP_TYPE,
    size: u64,
    initial_state: D3D12_RESOURCE_STATES,
) -> Result<ID3D12Resource, DecodeError> {
    // SAFETY: plain getter, no borrowed pointers.
    let heap_props = unsafe { device.GetCustomHeapProperties(0, heap_type) };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Alignment: 0,
        Width: size.max(1),
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: DXGI_FORMAT_UNKNOWN,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        Flags: D3D12_RESOURCE_FLAG_NONE,
    };
    let mut resource: Option<ID3D12Resource> = None;
    // SAFETY: `CreateCommittedResource` with a linear buffer on `heap_type`; caller-owned.
    unsafe {
        device
            .CreateCommittedResource(
                &raw const heap_props,
                D3D12_HEAP_FLAG_NONE,
                &raw const desc,
                initial_state,
                None,
                &raw mut resource,
            )
            .map_err(|_err| DecodeError::Backend)?;
    }
    resource.ok_or(DecodeError::Backend)
}
