using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using Mediaway.Common;
using Mediaway.Device;
using Mediaway.Device.Desktop;
using Mediaway.Device.Desktop.Interop;
using Xunit;
using Xunit.Abstractions;

namespace MediawayDeviceIntegrationTests;

/// <summary>
/// Skipped unless <c>MEDIAWAY_RUN_WINDOW_TESTS=1</c>: the test shows a real window on the
/// desktop the suite runs on, and a default run must not take over a developer's screen
/// (<c>docs/conventions/testing.md</c> § Tests that manipulate the desktop).
/// </summary>
public sealed class OptInWindowFactAttribute : FactAttribute
{
    public OptInWindowFactAttribute()
    {
        if (Environment.GetEnvironmentVariable("MEDIAWAY_RUN_WINDOW_TESTS") != "1")
        {
            Skip = "shows a real window; set MEDIAWAY_RUN_WINDOW_TESTS=1 to run";
        }
    }
}

/// <summary>
/// Window capture through the C# binding (adr/device/0005-window-capture-c-abi.md). The
/// non-hardware tests never touch a device or open a window — every case is rejected inside
/// the native config validation, before any backend call.
/// </summary>
public sealed partial class WindowCaptureTests
{
    private readonly ITestOutputHelper _output;

    public WindowCaptureTests(ITestOutputHelper output) => _output = output;

    private static readonly Rational Fps30 = new(1, 30);

    /// <summary>
    /// device.h mirrors: pointer-size-independent because every field is fixed-width. A
    /// reordered, resized or missing field fails here instead of silently corrupting the
    /// config the native side reads.
    /// </summary>
    [Fact]
    public void NativeDesktopCaptureConfig_LayoutMatchesTheHeader()
    {
        var c = new NativeDesktopCaptureConfig();
        Assert.Equal(88, Unsafe.SizeOf<NativeDesktopCaptureConfig>());
        Assert.Equal(0, Offset(ref c, ref Unsafe.As<NativeDesktopCaptureSourceKind, byte>(ref c.SourceKind)));
        Assert.Equal(4, Offset(ref c, ref Unsafe.As<uint, byte>(ref c.SourceIndex)));
        Assert.Equal(8, Offset(ref c, ref Unsafe.As<NativeRational, byte>(ref c.TimeBase)));
        Assert.Equal(24, Offset(ref c, ref Unsafe.As<GpuDeviceHandle, byte>(ref c.GpuDevice)));
        Assert.Equal(48, Offset(ref c, ref Unsafe.As<ulong, byte>(ref c.WindowHandle)));
        Assert.Equal(56, Offset(ref c, ref Unsafe.As<CursorCapture, byte>(ref c.Cursor)));
        Assert.Equal(60, Offset(ref c, ref Unsafe.As<WindowBorder, byte>(ref c.Border)));
        Assert.Equal(64, Offset(ref c, ref Unsafe.As<FrameDimensions, byte>(ref c.Dimensions)));
        Assert.Equal(68, Offset(ref c, ref Unsafe.As<uint, byte>(ref c.RegionX)));
        Assert.Equal(72, Offset(ref c, ref Unsafe.As<uint, byte>(ref c.RegionY)));
        Assert.Equal(76, Offset(ref c, ref Unsafe.As<uint, byte>(ref c.RegionWidth)));
        Assert.Equal(80, Offset(ref c, ref Unsafe.As<uint, byte>(ref c.RegionHeight)));
        Assert.Equal(84, Offset(ref c, ref c.RegionEnabled));
        Assert.Equal(4, Unsafe.SizeOf<CursorCapture>());
        Assert.Equal(4, Unsafe.SizeOf<WindowBorder>());
        Assert.Equal(4, Unsafe.SizeOf<FrameDimensions>());
    }

    private static int Offset(ref NativeDesktopCaptureConfig origin, ref byte field) =>
        (int)Unsafe.ByteOffset(ref Unsafe.As<NativeDesktopCaptureConfig, byte>(ref origin), ref field);

    /// <summary>A zero tail must mean "as before": whole window, no pointer, OS border, native size.</summary>
    [Fact]
    public void DefaultOptions_BuildAZeroTail()
    {
        var config = DesktopWindowCapture.BuildConfig(0x1234, Fps30, GpuDeviceHandle.None, options: null);
        Assert.Equal(NativeDesktopCaptureSourceKind.Window, config.SourceKind);
        Assert.Equal(0x1234UL, config.WindowHandle);
        Assert.Equal(CursorCapture.Excluded, config.Cursor);
        Assert.Equal(WindowBorder.Shown, config.Border);
        Assert.Equal(FrameDimensions.Native, config.Dimensions);
        Assert.Equal((byte)0, config.RegionEnabled);
        Assert.Equal(0u, config.RegionX + config.RegionY + config.RegionWidth + config.RegionHeight);
    }

    [Fact]
    public void Options_AreCarriedIntoTheConfig()
    {
        var options = new WindowCaptureOptions
        {
            Cursor = CursorCapture.Included,
            Border = WindowBorder.Hidden,
            Dimensions = FrameDimensions.EvenCropped,
            Region = new CaptureRegion(10, 8, 64, 48),
        };
        var config = DesktopWindowCapture.BuildConfig(1, Fps30, GpuDeviceHandle.None, options);
        Assert.Equal(CursorCapture.Included, config.Cursor);
        Assert.Equal(WindowBorder.Hidden, config.Border);
        Assert.Equal(FrameDimensions.EvenCropped, config.Dimensions);
        Assert.Equal((byte)1, config.RegionEnabled);
        Assert.Equal((10u, 8u, 64u, 48u), (config.RegionX, config.RegionY, config.RegionWidth, config.RegionHeight));
    }

    [Fact]
    public void RegionOutOfBounds_IsStatus14()
    {
        Assert.Equal(14, (int)MediawayDeviceStatus.RegionOutOfBounds);
        var ex = Assert.Throws<MediawayDeviceException>(
            () => MediawayDeviceException.ThrowIfError(MediawayDeviceStatus.RegionOutOfBounds));
        Assert.Equal(MediawayDeviceStatus.RegionOutOfBounds, ex.Status);
        Assert.DoesNotContain("Unknown", ex.Message);
    }

    [Fact]
    public void Open_WithHwndZero_IsInvalidInput()
    {
        var capture = DesktopWindowCapture.TryOpen(0, Fps30, GpuDeviceHandle.None, null, out var error);
        Assert.Null(capture);
        Assert.Equal(MediawayDeviceStatus.InvalidInput, error);
        Assert.Throws<MediawayDeviceException>(() => DesktopWindowCapture.Open(0, Fps30, GpuDeviceHandle.None));
    }

    [Fact]
    public void Open_WithAZeroSizedRegion_IsInvalidInput()
    {
        var options = new WindowCaptureOptions { Region = new CaptureRegion(0, 0, 0, 64) };
        var capture = DesktopWindowCapture.TryOpen(0x1234, Fps30, GpuDeviceHandle.None, options, out var error);
        Assert.Null(capture);
        Assert.Equal(MediawayDeviceStatus.InvalidInput, error);
    }

    /// <summary>
    /// DXGI cannot draw the pointer or crop, so Screen must refuse those requests rather than
    /// ignore them. The binding's Screen API does not expose them, so this goes through the
    /// native struct directly — the ABI contract, not a C# convenience.
    /// </summary>
    [Fact]
    public void ScreenConfig_WithCursorOrRegion_IsUnsupported_AndWindowOnlyOptionsAreInvalid()
    {
        var screen = new NativeDesktopCaptureConfig
        {
            SourceKind = NativeDesktopCaptureSourceKind.Screen,
            TimeBase = new NativeRational(Fps30),
        };

        var cursor = screen;
        cursor.Cursor = CursorCapture.Included;
        Assert.Equal(MediawayDeviceStatus.Unsupported, Open(cursor));

        var region = screen;
        region.RegionEnabled = 1;
        region.RegionWidth = 64;
        region.RegionHeight = 64;
        Assert.Equal(MediawayDeviceStatus.Unsupported, Open(region));

        var border = screen;
        border.Border = WindowBorder.Hidden;
        Assert.Equal(MediawayDeviceStatus.InvalidInput, Open(border));

        var dimensions = screen;
        dimensions.Dimensions = FrameDimensions.EvenCropped;
        Assert.Equal(MediawayDeviceStatus.InvalidInput, Open(dimensions));
    }

    private static MediawayDeviceStatus Open(NativeDesktopCaptureConfig config) =>
        NativeMethods.mediaway_desktop_capture_open(in config, out _);

    /// <summary>
    /// Real hardware: a real window, a real D3D11 device from the GPU factory, a real WGC
    /// session, every option through the C# surface. 335x250 makes WGC's capture size odd on
    /// both axes (it excludes the invisible resize borders), so even-cropping is observable.
    /// </summary>
    [OptInWindowFact]
    public void Window_Capture_AppliesEveryOptionAgainstARealWindow()
    {
        using var window = TestWindow.Create(335, 250);
        using var gpuDevice = GpuDevice.Create(new GpuDeviceOptions { VideoSupport = true });

        (uint Width, uint Height) native;
        using (var plain = DesktopWindowCapture.Open(window.Hwnd, Fps30, gpuDevice.Handle))
        {
            native = (plain.Width, plain.Height);
            Assert.False(plain.BorderHidden, "border = Shown must report not hidden");
            AssertOneFrame(plain, native);
        }

        _output.WriteLine($"native window capture: {native.Width}x{native.Height}");
        Assert.True(
            native.Width % 2 == 1 || native.Height % 2 == 1,
            $"the test window captured at {native}, even on both axes, so this proves nothing about cropping");

        var cropOptions = new WindowCaptureOptions
        {
            Cursor = CursorCapture.Included,
            Border = WindowBorder.Hidden,
            Dimensions = FrameDimensions.EvenCropped,
        };
        using (var cropped = DesktopWindowCapture.Open(window.Hwnd, Fps30, gpuDevice.Handle, cropOptions))
        {
            Assert.Equal((native.Width & ~1u, native.Height & ~1u), (cropped.Width, cropped.Height));
            Assert.Equal(0u, (cropped.Width | cropped.Height) & 1u);
            // Windows 11 build 22000+. Asserted, not skipped: a skip is how the border once went
            // unhidden with nobody noticing.
            Assert.True(cropped.BorderHidden, "the OS refused to hide the capture border");
            AssertOneFrame(cropped, (cropped.Width, cropped.Height));
        }

        // At the origin the frame pool is sized to the region (free); elsewhere it is one GPU
        // copy per frame. Both must deliver frames of exactly the region's size.
        foreach (var (x, y) in new[] { (0u, 0u), (10u, 8u) })
        {
            var options = new WindowCaptureOptions { Region = new CaptureRegion(x, y, 64, 48) };
            using var region = DesktopWindowCapture.Open(window.Hwnd, Fps30, gpuDevice.Handle, options);
            Assert.Equal((64u, 48u), (region.Width, region.Height));
            AssertOneFrame(region, (64, 48));
        }

        var tooBig = new WindowCaptureOptions
        {
            Region = new CaptureRegion(0, 0, native.Width + 100, native.Height + 100),
        };
        var failed = DesktopWindowCapture.TryOpen(window.Hwnd, Fps30, gpuDevice.Handle, tooBig, out var error);
        Assert.Null(failed);
        Assert.Equal(MediawayDeviceStatus.RegionOutOfBounds, error);
    }

    private static void AssertOneFrame(IDesktopVideoCapture capture, (uint Width, uint Height) expected)
    {
        var deadline = DateTime.UtcNow.AddSeconds(3);
        while (DateTime.UtcNow < deadline)
        {
            if (capture.TryPollFrame(out DesktopVideoFrame? frame))
            {
                using (frame)
                {
                    Assert.Equal(VideoFrameStorageKind.Gpu, frame!.StorageKind);
                    Assert.Equal(expected, (frame.Width, frame.Height));
                }

                capture.ReleaseFrame();
                return;
            }

            Thread.Sleep(15);
        }

        Assert.Fail("no frame within 3 s");
    }

    /// <summary>
    /// A real, visible top-level window. Uses the predefined <c>STATIC</c> class, so no window
    /// procedure has to be registered from managed code.
    /// </summary>
    private sealed partial class TestWindow : IDisposable
    {
        private const uint WsOverlappedWindow = 0x00CF0000;
        private const uint WsVisible = 0x10000000;
        private const int CwUseDefault = unchecked((int)0x80000000);

        [LibraryImport("user32.dll", EntryPoint = "CreateWindowExW", StringMarshalling = StringMarshalling.Utf16)]
        private static partial nint CreateWindowEx(
            uint exStyle, string className, string windowName, uint style, int x, int y, int width, int height,
            nint parent, nint menu, nint instance, nint param);

        [LibraryImport("user32.dll")]
        private static partial int DestroyWindow(nint hwnd);

        private TestWindow(nint hwnd) => Hwnd = hwnd;

        public nint Hwnd { get; }

        public static TestWindow Create(int width, int height)
        {
            var hwnd = CreateWindowEx(
                0, "STATIC", "mediaway csharp window capture", WsOverlappedWindow | WsVisible,
                CwUseDefault, CwUseDefault, width, height, 0, 0, 0, 0);
            Assert.NotEqual(nint.Zero, hwnd);
            return new TestWindow(hwnd);
        }

        public void Dispose() => _ = DestroyWindow(Hwnd);
    }
}
