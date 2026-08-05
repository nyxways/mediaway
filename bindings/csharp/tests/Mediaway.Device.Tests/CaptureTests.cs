using System.Runtime.InteropServices;
using Mediaway.Common;
using Mediaway.Device;
using Mediaway.Device.Audio;
using Mediaway.Device.Camera;
using Mediaway.Device.Desktop;
using Mediaway.Device.Hotplug;
using Xunit;
using Xunit.Abstractions;

namespace MediawayDeviceIntegrationTests;

/// <summary>
/// Exercises the real native <c>mediaway_ffi</c> library end-to-end — mirrors
/// <c>bindings/csharp/examples/CameraRecord.cs</c>'s capture-side scenario (open the
/// default camera + default microphone, read real frames) to verify the P/Invoke layer
/// against the actual ABI, not just that it compiles. This machine has a real, previously
/// hardware-verified USB camera and microphone, and a real display (Screen capture).
/// </summary>
public sealed partial class CaptureTests
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

    /// <summary>
    /// Exercises real Screen (DXGI Desktop Duplication) capture end to end from C# — the
    /// capture capability this suite couldn't cover before because Screen is Zero-Copy-only
    /// and requires a live caller-owned <c>ID3D11Device*</c> (ScreenRecord.cs's
    /// <c>OpenSharedD3D11Device</c> is a documented placeholder for exactly that reason; a
    /// real app brings its own device via Vortice or raw COM interop — <see cref="GpuDeviceHandle"/>
    /// deliberately does not construct one). This test supplies the device itself through a
    /// test-only raw <c>D3D11CreateDevice</c> P/Invoke (<see cref="NativeD3D11"/>), so the full
    /// open → poll → use → <see cref="IDesktopVideoCapture.ReleaseFrame"/> contract runs against
    /// real hardware like the Camera/Microphone/Hotplug tests do.
    /// </summary>
    [Fact]
    public void Screen_Open_WithRealD3D11Device_CapturesRealGpuFrames()
    {
        nint device = NativeD3D11.CreateHardwareDevice();
        try
        {
            using var capture = DesktopScreenCapture.Open(
                outputIndex: 0,
                frameRate: new Rational(1, 30),
                gpuDevice: GpuDeviceHandle.DirectX11(device));
            Assert.True(capture.Width > 0, "Screen capture did not negotiate a real width.");
            Assert.True(capture.Height > 0, "Screen capture did not negotiate a real height.");

            // DXGI DDA only delivers a frame when the desktop image or cursor changes — nudge
            // the cursor (restoring it afterwards) so an idle desktop still produces frames
            // deterministically, mirroring screen_mic_av_smoke.rs's nudge_cursor.
            NativeD3D11.NativePoint? cursorOrigin = NativeD3D11.TryGetCursorPos();
            using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(3));
            int frameCount = 0;
            bool toggle = false;
            try
            {
                while (!cts.IsCancellationRequested && frameCount < 3)
                {
                    toggle = !toggle;
                    NativeD3D11.NudgeCursor(toggle, cursorOrigin);

                    if (capture.TryPollFrame(out DesktopVideoFrame? frame))
                    {
                        using (frame)
                        {
                            Assert.Equal(capture.Width, frame!.Width);
                            Assert.Equal(capture.Height, frame.Height);
                            Assert.True(
                                frame.StorageKind == VideoFrameStorageKind.Gpu,
                                $"Screen frame was not GPU-backed: {frame.StorageKind}.");
                            Assert.NotEqual(nint.Zero, frame.GpuBuffer.NativeA);
                            _output.WriteLine(
                                $"Screen frame {frameCount + 1}: {frame.Width}x{frame.Height}, {frame.StorageKind}, GpuBuffer.NativeA=0x{frame.GpuBuffer.NativeA:X}");
                        }

                        // Done reading the frame's GpuBuffer — invalidate it before polling again
                        // (IDesktopVideoCapture's documented contract; frame.Dispose is a no-op
                        // for the GPU case, ReleaseFrame is the real release point).
                        capture.ReleaseFrame();
                        frameCount++;
                    }
                    else
                    {
                        Thread.Sleep(15);
                    }
                }
            }
            finally
            {
                NativeD3D11.RestoreCursor(cursorOrigin);
            }

            Assert.True(frameCount > 0, "Received zero real frames from the screen within 3 s.");
        }
        finally
        {
            // D3D11CreateDevice returned the device with refcount 1; the capture session (which
            // only borrows it) is already disposed above, so this is the final release.
            Marshal.Release(device);
        }
    }

    /// <summary>
    /// Test-only raw COM interop that creates a live hardware <c>ID3D11Device*</c> via
    /// <c>d3d11.dll</c>'s <c>D3D11CreateDevice</c> — the minimal device Screen capture needs,
    /// same native call shape as crates/mediaway/tests/screen_mic_av_smoke.rs's
    /// <c>open_shared_d3d11_device</c> (hardware driver type, default feature-level set,
    /// <c>D3D11_SDK_VERSION</c>). Deliberately not a general-purpose D3D11 wrapper —
    /// <see cref="GpuDeviceHandle"/> documents device construction as a Direct3D interop
    /// concern this binding does not wrap, and the shipped ScreenRecord.cs example keeps a
    /// placeholder for the same reason. The caller owns the returned pointer (refcount 1) and
    /// releases it with <see cref="Marshal.Release"/> once the session borrowing it is gone.
    /// </summary>
    private static partial class NativeD3D11
    {
        private const uint D3DDriverTypeHardware = 1;
        private const uint D3D11CreateDeviceVideoSupport = 0x800;
        private const uint D3D11SdkVersion = 7;

        [LibraryImport("d3d11.dll")]
        private static partial int D3D11CreateDevice(
            nint pAdapter,
            uint driverType,
            nint software,
            uint flags,
            nint pFeatureLevels,
            uint featureLevels,
            uint sdkVersion,
            out nint ppDevice,
            nint pFeatureLevel,
            nint ppImmediateContext);

        [LibraryImport("user32.dll")]
        private static partial int GetCursorPos(out NativePoint point);

        [LibraryImport("user32.dll")]
        private static partial int SetCursorPos(int x, int y);

        [StructLayout(LayoutKind.Sequential)]
        public struct NativePoint
        {
            public int X;
            public int Y;
        }

        /// <summary>Creates a hardware <c>ID3D11Device*</c> (refcount 1); the caller releases it.</summary>
        public static nint CreateHardwareDevice()
        {
            int hr = D3D11CreateDevice(
                nint.Zero,       // pAdapter: NULL selects the default adapter.
                D3DDriverTypeHardware,
                nint.Zero,       // Software: NULL (ignored for the hardware driver type).
                D3D11CreateDeviceVideoSupport,
                nint.Zero,       // pFeatureLevels: NULL lets the runtime pick its default set.
                0,               // FeatureLevels: ignored while pFeatureLevels is NULL.
                D3D11SdkVersion,
                out nint device,
                nint.Zero,       // pFeatureLevel: not requested.
                nint.Zero);      // ppImmediateContext: not requested.

            if (hr < 0)
            {
                throw new InvalidOperationException($"D3D11CreateDevice failed with HRESULT 0x{hr:X8}.");
            }

            return device;
        }

        /// <summary>Current cursor position, or <see langword="null"/> if the session can't report one.</summary>
        public static NativePoint? TryGetCursorPos() =>
            GetCursorPos(out NativePoint point) != 0 ? point : null;

        /// <summary>
        /// Moves the cursor one pixel left/right of its origin and back — DDA delivers a new
        /// frame on cursor move, so this keeps frames flowing on an otherwise-idle desktop.
        /// </summary>
        public static void NudgeCursor(bool toggle, NativePoint? origin)
        {
            if (origin is not { } o)
            {
                return; // GetCursorPos failed — poll without nudging.
            }

            _ = SetCursorPos(o.X + (toggle ? 1 : -1), o.Y);
        }

        /// <summary>Restores the cursor to where the test found it.</summary>
        public static void RestoreCursor(NativePoint? origin)
        {
            if (origin is { } o)
            {
                _ = SetCursorPos(o.X, o.Y);
            }
        }
    }
}
