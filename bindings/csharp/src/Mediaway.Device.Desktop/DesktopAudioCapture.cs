using Mediaway.Common;
using Mediaway.Device.Desktop.Interop;

namespace Mediaway.Device.Desktop;

/// <summary>
/// Opens an audio capture session against what the desktop is already rendering
/// (loopback / process-loopback) — "what's playing," not a real input device. See
/// <c>Mediaway.Device.Audio.Microphone</c> for a real input device.
/// </summary>
public static class DesktopAudioCapture
{
    /// <summary>Captures the default render endpoint's output (system-wide "what you hear").</summary>
    /// <param name="sampleRateTimeBase">Timebase for the frames this session will negotiate.</param>
    /// <exception cref="CaptureUnavailableException">No supported capture backend is compiled in here.</exception>
    public static IDesktopAudioCapture OpenLoopback(Rational sampleRateTimeBase)
    {
        var config = BuildLoopbackConfig(sampleRateTimeBase);
        return OpenFrom(config);
    }

    /// <summary>Non-throwing form of <see cref="OpenLoopback"/>.</summary>
    public static IDesktopAudioCapture? TryOpenLoopback(Rational sampleRateTimeBase, out MediawayDeviceStatus? error) =>
        TryOpenFrom(BuildLoopbackConfig(sampleRateTimeBase), out error);

    /// <summary>
    /// Per-process loopback — Windows 10 2004+. With
    /// <paramref name="includeTargetProcessTree"/> <c>true</c>, captures the audio rendered
    /// by <paramref name="processId"/> and its descendants; with <c>false</c>, captures
    /// everything the desktop renders <em>except</em> that process tree. Windows offers no
    /// "target process alone" mode, so <c>false</c> is the complement of <c>true</c>, not a
    /// narrowing of it. Capture is IEEE float at a fixed 48 kHz stereo layout on the Windows
    /// backend (mix-format queries are unsupported for this mode).
    /// </summary>
    /// <exception cref="CaptureUnavailableException">No supported capture backend is compiled in here.</exception>
    public static IDesktopAudioCapture OpenProcessLoopback(
        uint processId, bool includeTargetProcessTree, Rational sampleRateTimeBase)
    {
        var config = BuildProcessLoopbackConfig(processId, includeTargetProcessTree, sampleRateTimeBase);
        return OpenFrom(config);
    }

    /// <summary>Non-throwing form of <see cref="OpenProcessLoopback"/>.</summary>
    public static IDesktopAudioCapture? TryOpenProcessLoopback(
        uint processId, bool includeTargetProcessTree, Rational sampleRateTimeBase, out MediawayDeviceStatus? error) =>
        TryOpenFrom(BuildProcessLoopbackConfig(processId, includeTargetProcessTree, sampleRateTimeBase), out error);

    private static IDesktopAudioCapture OpenFrom(NativeDesktopAudioCaptureConfig config)
    {
        var status = NativeMethods.mediaway_desktop_audio_capture_open(in config, out nint handle);
        MediawayDeviceException.ThrowIfError(status);
        return new DesktopAudioCaptureSession(DesktopAudioCaptureHandle.Wrap(handle));
    }

    private static IDesktopAudioCapture? TryOpenFrom(NativeDesktopAudioCaptureConfig config, out MediawayDeviceStatus? error)
    {
        var status = NativeMethods.mediaway_desktop_audio_capture_open(in config, out nint handle);
        if (status != MediawayDeviceStatus.Ok)
        {
            error = status;
            return null;
        }

        error = null;
        return new DesktopAudioCaptureSession(DesktopAudioCaptureHandle.Wrap(handle));
    }

    private static NativeDesktopAudioCaptureConfig BuildLoopbackConfig(Rational sampleRateTimeBase) => new()
    {
        SourceKind = NativeDesktopAudioSourceKind.Loopback,
        DeviceIndex = 0,
        ProcessId = 0,
        IncludeTargetProcessTree = 0,
        TimeBase = new NativeRational(sampleRateTimeBase),
        SampleFormat = SampleFormat.F32, // Only format the real Windows backend accepts today.
    };

    private static NativeDesktopAudioCaptureConfig BuildProcessLoopbackConfig(
        uint processId, bool includeTargetProcessTree, Rational sampleRateTimeBase) => new()
    {
        SourceKind = NativeDesktopAudioSourceKind.ProcessLoopback,
        DeviceIndex = 0,
        ProcessId = processId,
        IncludeTargetProcessTree = includeTargetProcessTree ? (byte)1 : (byte)0,
        TimeBase = new NativeRational(sampleRateTimeBase),
        SampleFormat = SampleFormat.F32,
    };
}
