using System.Buffers;
#if NET8_0_OR_GREATER
using System.Runtime.CompilerServices;
using System.Threading.Channels;
#endif
using Mediaway.Common.Interop;
using Mediaway.Device.Desktop.Interop;

namespace Mediaway.Device.Desktop;

internal sealed class DesktopAudioCaptureSession : IDesktopAudioCapture
{
#if NET8_0_OR_GREATER
    /// <summary>
    /// How often to re-poll when the last poll found no samples ready. Low enough to add no
    /// perceptible latency; high enough to not busy-spin.
    /// </summary>
    private static readonly TimeSpan PollInterval = TimeSpan.FromMilliseconds(2);
#endif

    private readonly DesktopAudioCaptureHandle _handle;

    internal DesktopAudioCaptureSession(DesktopAudioCaptureHandle handle) => _handle = handle;

#if NET8_0_OR_GREATER
    public async IAsyncEnumerable<DesktopAudioFrame> ReadFramesAsync(
        [EnumeratorCancellation] CancellationToken cancellationToken = default)
    {
        var channel = Channel.CreateBounded<DesktopAudioFrame>(
            new BoundedChannelOptions(4) { FullMode = BoundedChannelFullMode.Wait });
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

    private async Task PumpAsync(ChannelWriter<DesktopAudioFrame> writer, CancellationToken cancellationToken)
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

    public bool TryPollFrame(out DesktopAudioFrame? frame)
    {
        var status = NativeMethods.mediaway_desktop_audio_capture_poll_frame(_handle, out var native, out byte hasFrame);
        MediawayDeviceException.ThrowIfError(status);
        if (hasFrame == 0)
        {
            frame = null;
            return false;
        }

        frame = ToManaged(native);
        return true;
    }

    private static DesktopAudioFrame ToManaged(NativeDesktopAudioFrame native)
    {
        IMemoryOwner<byte> owner = native.DataLen == 0
            ? EmptyMemoryOwner<byte>.Instance
            : new NativeOwnedMemoryManager(
                native.Data, native.DataLen,
                static (ptr, len) =>
                {
                    var owned = new NativeDesktopAudioFrame { Data = ptr, DataLen = len };
                    NativeMethods.mediaway_desktop_audio_frame_free(ref owned);
                });

        return new DesktopAudioFrame(owner)
        {
            Pts = native.Pts,
            Duration = native.Duration,
            SampleRate = native.SampleRate,
            Channels = native.Channels,
            SampleFormat = native.SampleFormat,
            Data = owner.Memory,
        };
    }

    public void Dispose() => _handle.Dispose();
}
