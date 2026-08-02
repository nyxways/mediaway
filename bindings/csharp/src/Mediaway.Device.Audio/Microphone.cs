using Mediaway.Common;
using Mediaway.Device.Audio.Interop;

namespace Mediaway.Device.Audio;

/// <summary>Opens an audio capture session against the default microphone endpoint.</summary>
public static class Microphone
{
    /// <param name="sampleRateTimeBase">Timebase for the frames this session will negotiate.</param>
    /// <exception cref="CaptureUnavailableException">No supported capture backend is compiled in here.</exception>
    public static IAudioCapture Open(Rational sampleRateTimeBase)
    {
        var config = BuildConfig(sampleRateTimeBase);
        var status = NativeMethods.mediaway_audio_capture_open(in config, out nint handle);
        MediawayDeviceException.ThrowIfError(status);
        return new AudioCaptureSession(AudioCaptureHandle.Wrap(handle));
    }

    /// <summary>Non-throwing form of <see cref="Open"/> — returns <see langword="null"/> and the failure status instead of throwing.</summary>
    public static IAudioCapture? TryOpen(Rational sampleRateTimeBase, out MediawayDeviceStatus? error)
    {
        var config = BuildConfig(sampleRateTimeBase);
        var status = NativeMethods.mediaway_audio_capture_open(in config, out nint handle);
        if (status != MediawayDeviceStatus.Ok)
        {
            error = status;
            return null;
        }

        error = null;
        return new AudioCaptureSession(AudioCaptureHandle.Wrap(handle));
    }

    private static NativeAudioCaptureConfig BuildConfig(Rational sampleRateTimeBase) => new()
    {
        DeviceIndex = 0,
        TimeBase = new NativeRational(sampleRateTimeBase),
        SampleFormat = SampleFormat.F32, // Only format the real Windows backend accepts today.
    };
}
