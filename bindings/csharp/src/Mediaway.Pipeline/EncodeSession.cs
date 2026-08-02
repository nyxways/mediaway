using System.Buffers;
using Mediaway.Common;
using Mediaway.Common.Interop;
using Mediaway.Pipeline.Interop;

namespace Mediaway.Pipeline;

/// <summary>
/// A live encode session — obtained from <see cref="Open"/>, which registers the encoder's
/// stream as an MP4 track and consumes the <see cref="AutoVideoEncoder"/> unconditionally
/// (success or failure).
/// </summary>
public sealed class EncodeSession : IDisposable
{
    private readonly EncodeSessionHandle _handle;

    private EncodeSession(EncodeSessionHandle handle) => _handle = handle;

    /// <summary>
    /// Register <paramref name="encoder"/>'s stream as an MP4 track and begin streaming.
    /// Consumes <paramref name="encoder"/> unconditionally — it must not be used again after
    /// this call, success or failure.
    /// </summary>
    public static EncodeSession Open(AutoVideoEncoder encoder)
    {
        var status = NativeMethods.mediaway_encode_session_open(encoder.Handle, out nint session);
        // Consumed unconditionally by the native call (even on failure) — invalidate now so
        // a later Dispose()/finalize on `encoder` never double-closes an already-freed pointer.
        encoder.Handle.SetHandleAsInvalid();

        MediawayPipelineException.ThrowIfError(status);
        return new EncodeSession(EncodeSessionHandle.Wrap(session));
    }

    /// <summary>Push one CPU-backed frame and drain any packets it produces into the muxer.</summary>
    public unsafe void WriteFrame(VideoFrame frame)
    {
        using var pin = frame.Data.Pin();
        var native = new NativeVideoFrame
        {
            Pts = frame.Pts,
            Duration = frame.Duration,
            Width = frame.Width,
            Height = frame.Height,
            PixelFormat = frame.PixelFormat,
            StorageKind = VideoFrameStorageKind.Cpu,
            RawBytes = frame.Data.IsEmpty ? null : (byte*)pin.Pointer,
            RawBytesLen = (nuint)frame.Data.Length,
        };
        MediawayPipelineException.ThrowIfError(NativeMethods.mediaway_encode_session_write_frame(_handle, in native));
    }

    /// <summary>
    /// Push one GPU-backed frame and drain any packets it produces into the muxer. Only
    /// valid on a session opened from a <see cref="VideoEncodeConfig"/> whose
    /// <see cref="VideoEncodeConfig.GpuDevice"/> is a real device — see
    /// <see cref="GpuVideoFrame"/>'s own doc comment for the borrowed-texture lifetime
    /// contract this call relies on.
    /// </summary>
    public unsafe void WriteGpuFrame(GpuVideoFrame frame)
    {
        var native = new NativeVideoFrame
        {
            Pts = frame.Pts,
            Duration = frame.Duration,
            Width = frame.Width,
            Height = frame.Height,
            PixelFormat = frame.PixelFormat,
            StorageKind = VideoFrameStorageKind.Gpu,
            RawBytes = null,
            RawBytesLen = 0,
            GpuBuffer = frame.GpuBuffer,
        };
        MediawayPipelineException.ThrowIfError(NativeMethods.mediaway_encode_session_write_frame(_handle, in native));
    }

    /// <summary>
    /// Flush the encoder and muxer, returning the complete fMP4 byte stream — Zero-Copy over
    /// the native buffer; dispose the returned owner to release it. Consumes this session
    /// unconditionally (success or failure); it must not be used again afterward.
    /// </summary>
    public IMemoryOwner<byte> Finish()
    {
        var status = NativeMethods.mediaway_encode_session_finish(_handle, out nint data, out nuint len);
        // Consumed unconditionally by the native call — invalidate now so a later
        // Dispose()/finalize never double-closes an already-freed pointer.
        _handle.SetHandleAsInvalid();

        MediawayPipelineException.ThrowIfError(status);

        if (data == 0 || len == 0)
        {
            return EmptyMemoryOwner<byte>.Instance;
        }

        return new NativeOwnedMemoryManager(
            data, len, static (ptr, l) => NativeMethods.mediaway_pipeline_ffi_buffer_free(ptr, l));
    }

    /// <summary>
    /// Releases the native session. A no-op once <see cref="Finish"/> has consumed it.
    /// </summary>
    public void Dispose() => _handle.Dispose();
}
