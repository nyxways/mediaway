using System.Buffers;
#if NET8_0_OR_GREATER
using System.Runtime.CompilerServices;
using System.Threading.Channels;
#endif
using Mediaway.Common.Interop;
using Mediaway.Device.Camera.Interop;

namespace Mediaway.Device.Camera;

internal sealed class CameraCaptureSession : IVideoCapture
{
#if NET8_0_OR_GREATER
    /// <summary>
    /// How often to re-poll when the last poll found no frame ready. Low enough to add no
    /// perceptible latency against a real camera's frame rate; high enough to not busy-spin.
    /// </summary>
    private static readonly TimeSpan PollInterval = TimeSpan.FromMilliseconds(2);
#endif

    private readonly CameraCaptureHandle _handle;

    private CameraCaptureSession(CameraCaptureHandle handle, uint width, uint height)
    {
        _handle = handle;
        Width = width;
        Height = height;
    }

    public uint Width { get; }

    public uint Height { get; }

    internal static CameraCaptureSession OpenFrom(CameraCaptureHandle handle)
    {
        var status = NativeMethods.mediaway_camera_capture_geometry(handle, out uint width, out uint height);
        MediawayDeviceException.ThrowIfError(status);
        return new CameraCaptureSession(handle, width, height);
    }

#if NET8_0_OR_GREATER
    public async IAsyncEnumerable<VideoFrame> ReadFramesAsync(
        [EnumeratorCancellation] CancellationToken cancellationToken = default)
    {
        var channel = Channel.CreateBounded<VideoFrame>(
            new BoundedChannelOptions(2) { FullMode = BoundedChannelFullMode.Wait });
        var pumpTask = PumpAsync(channel.Writer, cancellationToken);

        try
        {
            await foreach (var frame in channel.Reader.ReadAllAsync(cancellationToken).ConfigureAwait(false))
            {
                yield return frame;
            }
        }
        finally
        {
            await pumpTask.ConfigureAwait(false);
        }
    }

    private async Task PumpAsync(ChannelWriter<VideoFrame> writer, CancellationToken cancellationToken)
    {
        try
        {
            while (!cancellationToken.IsCancellationRequested)
            {
                if (TryPollFrame(out var frame))
                {
                    await writer.WriteAsync(frame!, cancellationToken).ConfigureAwait(false);
                }
                else
                {
                    await Task.Delay(PollInterval, cancellationToken).ConfigureAwait(false);
                }
            }
        }
        catch (OperationCanceledException)
        {
            // Expected: ReadFramesAsync's consumer stopped or the token fired.
        }
        finally
        {
            writer.TryComplete();
        }
    }
#endif

    public unsafe bool TryPollFrame(out VideoFrame? frame)
    {
        var status = NativeMethods.mediaway_camera_capture_poll_frame(_handle, out var native, out byte hasFrame);
        MediawayDeviceException.ThrowIfError(status);
        if (hasFrame == 0)
        {
            frame = null;
            return false;
        }

        frame = ToManaged(native);

        // Must be called before the next frame-acquiring poll (documented contract) — done
        // eagerly here, right after the frame is converted, well before any caller's next
        // TryPollFrame call.
        MediawayDeviceException.ThrowIfError(NativeMethods.mediaway_camera_capture_release_frame(_handle));

        return true;
    }

    private static VideoFrame ToManaged(NativeCameraFrame native)
    {
        IMemoryOwner<byte> owner = native.DataLen == 0
            ? EmptyMemoryOwner<byte>.Instance
            : new NativeOwnedMemoryManager(
                native.Data, native.DataLen,
                static (ptr, len) =>
                {
                    var owned = new NativeCameraFrame { Data = ptr, DataLen = len };
                    NativeMethods.mediaway_camera_frame_free(ref owned);
                });

        return new VideoFrame(owner)
        {
            Pts = native.Pts,
            Duration = native.Duration,
            Width = native.Width,
            Height = native.Height,
            PixelFormat = native.PixelFormat,
            Data = owner.Memory,
        };
    }

    public void Dispose() => _handle.Dispose();
}
