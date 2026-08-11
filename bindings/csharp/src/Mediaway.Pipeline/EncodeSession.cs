using System.Buffers;
using Mediaway.Common;
using Mediaway.Common.Interop;
using Mediaway.Device.Camera;
using Mediaway.Device.Desktop;
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
    /// Poll one frame from <paramref name="capture"/> and, if one was ready, push it straight
    /// into the encoder in a single native call — no intermediate frame struct crosses the
    /// FFI boundary, and Zero-Copy for GPU-backed frames
    /// (adr/pipeline/0005-capture-encode-bridge-c-abi.md). Returns <see langword="false"/>
    /// (a no-op) when no frame was ready yet, mirroring
    /// <see cref="IVideoCapture.TryPollFrame"/>'s own contract.
    /// </summary>
    /// <param name="capture">Must be a session opened via <c>Camera.Open</c>/<c>TryOpen</c>.</param>
    public bool WriteFrameFromCameraCapture(IVideoCapture capture)
    {
        if (capture is not CameraCaptureSession session)
        {
            throw new ArgumentException(
                "capture must be a session opened via Camera.Open()/TryOpen().", nameof(capture));
        }

        var status = NativeMethods.mediaway_encode_session_write_frame_from_camera_capture(
            _handle, session.Handle, out byte wroteFrame);
        MediawayPipelineException.ThrowIfError(status);
        return wroteFrame != 0;
    }

    /// <summary>
    /// Poll one frame from <paramref name="capture"/> and, if one was ready, push it straight
    /// into the encoder in a single native call — same bridge as
    /// <see cref="WriteFrameFromCameraCapture"/>, but for Screen's GPU-only frames
    /// (adr/pipeline/0005-capture-encode-bridge-c-abi.md). Only valid on a session opened
    /// from a <see cref="VideoEncodeConfig"/> whose <see cref="VideoEncodeConfig.GpuDevice"/>
    /// is a real device, sharing the same GPU device the capture itself was opened with.
    /// </summary>
    /// <param name="capture">Must be a session opened via <c>DesktopScreenCapture.Open</c>/<c>TryOpen</c>.</param>
    public bool WriteFrameFromDesktopCapture(IDesktopVideoCapture capture)
    {
        if (capture is not DesktopVideoCaptureSession session)
        {
            throw new ArgumentException(
                "capture must be a session opened via DesktopScreenCapture.Open()/TryOpen().", nameof(capture));
        }

        var status = NativeMethods.mediaway_encode_session_write_frame_from_desktop_capture(
            _handle, session.Handle, out byte wroteFrame);
        MediawayPipelineException.ThrowIfError(status);
        return wroteFrame != 0;
    }

    /// <summary>
    /// Retarget the live CBR bitrate ceiling, taking effect from the next
    /// <see cref="WriteFrame"/>/<see cref="WriteGpuFrame"/> call — no session reopen, no
    /// dropped frames. Only meaningful for a session whose underlying encoder was opened
    /// with <see cref="VideoEncodeConfig.RateControl"/> set; a fixed-QP session throws
    /// <see cref="MediawayPipelineException"/> (native status <c>UNSUPPORTED</c>) — which,
    /// today, is every session <see cref="AutoVideoEncoder.Open"/> can actually produce
    /// (see <see cref="VideoEncodeConfig.GopSize"/>'s doc comment).
    /// </summary>
    public void SetBitrate(uint bitrateBps) =>
        MediawayPipelineException.ThrowIfError(NativeMethods.mediaway_encode_session_set_bitrate(_handle, bitrateBps));

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
