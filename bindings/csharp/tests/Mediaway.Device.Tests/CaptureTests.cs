using Mediaway.Common;
using Mediaway.Device;
using Mediaway.Device.Audio;
using Mediaway.Device.Camera;
using Mediaway.Device.Hotplug;
using Xunit;
using Xunit.Abstractions;

namespace MediawayDeviceIntegrationTests;

/// <summary>
/// Exercises the real native <c>mediaway_device_ffi</c> library end-to-end — mirrors
/// <c>bindings/csharp/examples/CameraRecord.cs</c>'s capture-side scenario (open the
/// default camera + default microphone, read real frames) to verify the P/Invoke layer
/// against the actual ABI, not just that it compiles. This machine has a real, previously
/// hardware-verified USB camera and microphone.
/// </summary>
public sealed class CaptureTests
{
    private readonly ITestOutputHelper _output;

    public CaptureTests(ITestOutputHelper output) => _output = output;

    [Fact]
    public async Task Camera_Open_NegotiatesGeometry_AndCapturesRealFrames()
    {
        using var video = Camera.Open(deviceIndex: 0, frameRate: new Rational(1, 30));
        Assert.True(video.Width > 0, "Camera did not negotiate a real width.");
        Assert.True(video.Height > 0, "Camera did not negotiate a real height.");

        using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(3));
        int frameCount = 0;
        try
        {
            await foreach (var frame in video.ReadFramesAsync(cts.Token))
            {
                using (frame)
                {
                    Assert.True(frame.Data.Length > 0, "Captured frame had no data.");
                    Assert.Equal(video.Width, frame.Width);
                    Assert.Equal(video.Height, frame.Height);
                    frameCount++;
                    if (frameCount >= 3)
                    {
                        break;
                    }
                }
            }
        }
        catch (OperationCanceledException)
        {
            // Expected if the camera is slower than the 3 s deadline.
        }

        Assert.True(frameCount > 0, "Received zero real frames from the camera within 3 s.");
    }

    [Fact]
    public void Camera_TryOpen_WithOutOfRangeIndex_FailsGracefully()
    {
        var result = Camera.TryOpen(deviceIndex: 999, frameRate: new Rational(1, 30), out var error);
        Assert.Null(result);
        Assert.NotNull(error);
        _output.WriteLine($"Camera.TryOpen(999) failed with: {error}");
    }

    [Fact]
    public async Task Microphone_Open_CapturesRealAudioFrames()
    {
        using var audio = Microphone.Open(new Rational(1, 48_000));

        using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(3));
        int frameCount = 0;
        try
        {
            await foreach (var frame in audio.ReadFramesAsync(cts.Token))
            {
                using (frame)
                {
                    Assert.True(frame.Data.Length > 0, "Captured audio frame had no data.");
                    Assert.True(frame.SampleRate > 0, "Microphone did not negotiate a real sample rate.");
                    frameCount++;
                    if (frameCount >= 3)
                    {
                        break;
                    }
                }
            }
        }
        catch (OperationCanceledException)
        {
            // Expected if fewer than 3 chunks arrive within the 3 s deadline.
        }

        Assert.True(frameCount > 0, "Received zero real audio frames from the microphone within 3 s.");
    }

    [Fact]
    public void DeviceHotplug_Open_PollEvent_DoesNotThrow()
    {
        using var hotplug = DeviceHotplug.Open(DeviceKind.Microphone);

        // No hardware is being plugged/unplugged during this test, so no event is expected —
        // this only verifies that opening and polling a real watcher succeeds without error.
        var evt = hotplug.PollEvent();
        Assert.Null(evt);
    }

    /// <summary>
    /// Exercises the real native push-mode callback
    /// (<c>mediaway_device_hotplug_register_callback</c>/<c>_unregister_callback</c>) end
    /// to end — registration, mode exclusivity against <see cref="DeviceHotplug.PollEvent"/>,
    /// and clean unregistration/teardown — against the real Windows backend. No hardware is
    /// plugged/unplugged during this test, so no event is expected to actually fire; this
    /// verifies the GCHandle/UnmanagedCallersOnly wiring itself doesn't crash or leak, not
    /// event delivery content (that needs a real plug/unplug, out of scope for CI).
    /// </summary>
    [Fact]
    public void DeviceHotplug_DeviceChanged_RegistersAndUnregistersRealNativeCallback()
    {
        using var hotplug = DeviceHotplug.Open(DeviceKind.Microphone, DeviceKind.Loopback);

        void Handler(object? sender, DeviceChangedEventArgs e) =>
            _output.WriteLine($"unexpected event during test: {e.Kind} {e.ChangeType} ({e.DeviceId})");

        hotplug.DeviceChanged += Handler;

        // Mode exclusivity: polling while a callback is registered must fail with
        // CallbackModeActive, not silently drain nothing (adr/0002-callback-event-delivery.md §4).
        var ex = Assert.Throws<MediawayDeviceException>(() => hotplug.PollEvent());
        Assert.Equal(MediawayDeviceStatus.CallbackModeActive, ex.Status);

        // Give the real bridging thread a moment to actually be running before tearing down —
        // this is exercising a real background thread's startup, not just the registration call.
        Thread.Sleep(100);

        hotplug.DeviceChanged -= Handler;

        // Back to poll mode after the last handler is removed — no exception expected.
        var evt = hotplug.PollEvent();
        Assert.Null(evt);
    }
}
