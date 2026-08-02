using Mediaway.Pipeline.Interop;

namespace Mediaway.Pipeline;

/// <summary>
/// The best available video encoder for a <see cref="VideoEncodeConfig"/> on the current
/// platform — an intermediate handle. Register its stream with a muxer and start encoding
/// via <see cref="EncodeSession.Open"/>, which consumes this instance.
/// </summary>
public sealed class AutoVideoEncoder : IDisposable
{
    internal AutoEncoderHandle Handle { get; }

    private AutoVideoEncoder(AutoEncoderHandle handle) => Handle = handle;

    /// <summary>
    /// Open the best available video encoder for <paramref name="config"/> on this platform.
    /// </summary>
    /// <exception cref="EncoderUnavailableException">
    /// No supported encoder backend is compiled in here — an expected, graceful outcome to
    /// catch and handle, not a bug.
    /// </exception>
    public static AutoVideoEncoder Open(VideoEncodeConfig config)
    {
        var native = new NativeAutoVideoEncodeConfig
        {
            Codec = config.Codec,
            Width = config.Width,
            Height = config.Height,
            TimeBase = new NativeRational(config.TimeBase),
            BitrateBps = config.BitrateBps,
            PixelFormat = config.PixelFormat,
            GpuDevice = config.GpuDevice,
        };

        var status = NativeMethods.mediaway_auto_encoder_open(in native, out nint encoder);
        MediawayPipelineException.ThrowIfError(status);
        return new AutoVideoEncoder(AutoEncoderHandle.Wrap(encoder));
    }

    /// <summary>
    /// Releases the native encoder. A no-op once <see cref="EncodeSession.Open"/> has
    /// consumed it — dispose the returned <see cref="EncodeSession"/> instead.
    /// </summary>
    public void Dispose() => Handle.Dispose();
}
