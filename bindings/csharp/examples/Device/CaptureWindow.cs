// CaptureWindow.cs — Mediaway C# quick start (single-window capture, Zero-Copy GPU frames).
//
// The `Mediaway.Common`/`Mediaway.Device`/`Mediaway.Device.Desktop` packages this targets are
// real — see bindings/csharp/src/ and crates/mediaway-ffi/adr/device/0005-window-capture-c-abi.md.
// Mirrors examples/device/capture_window.rs: open one window by HWND, poll ~2 s of frames,
// print their size, close. No encoding here — see Pipeline/ScreenRecord.cs for the
// capture-to-encode bridge, which accepts a window session exactly like a screen session.
//
// Like Screen, Window capture is Zero-Copy only: it needs a live `ID3D11Device*` that the caller
// keeps alive for the whole session, which Mediaway.Device.GpuDevice.Create provides. Windows only
// (WGC); on other platforms `Open` throws MediawayDeviceException(Unsupported).
//
// Run (the process name without ".exe", e.g. notepad):
//     dotnet run -- notepad

using System.Diagnostics;
using Mediaway.Common;
using Mediaway.Device;
using Mediaway.Device.Desktop;

const int CaptureSeconds = 2;

string processName = args.Length > 0 ? args[0] : "notepad";
Process? target = Process.GetProcessesByName(processName).FirstOrDefault(p => p.MainWindowHandle != nint.Zero);
if (target is null)
{
    Console.WriteLine($"CaptureWindow: no '{processName}' process with a visible main window — nothing to capture.");
    return;
}

using GpuDevice gpu = GpuDevice.Create(new GpuDeviceOptions { VideoSupport = true });

// Every option is optional; each default is the previous behaviour (whole window, no pointer,
// OS default border, native size). EvenCropped trims an odd axis so any hardware encoder accepts
// the frames, at no cost. A `Region` away from the window's origin costs one GPU copy per frame.
var options = new WindowCaptureOptions
{
    Cursor = CursorCapture.Included,
    Border = WindowBorder.Hidden,
    Dimensions = FrameDimensions.EvenCropped,
};

IDesktopWindowCapture capture;
try
{
    capture = DesktopWindowCapture.Open(target.MainWindowHandle, new Rational(1, 30), gpu.Handle, options);
}
catch (MediawayDeviceException ex)
{
    Console.WriteLine($"CaptureWindow: could not open the window ({ex.Status}) — {ex.Message}");
    return;
}

using (capture)
{
    // A refusal to hide the border is not an error at open; this is how a caller finds out.
    Console.WriteLine(
        $"CaptureWindow: {capture.Width}x{capture.Height}, border hidden by the OS: {capture.BorderHidden}");

    int frames = 0;
    var deadline = DateTime.UtcNow.AddSeconds(CaptureSeconds);
    while (DateTime.UtcNow < deadline)
    {
        if (capture.TryPollFrame(out DesktopVideoFrame? frame))
        {
            using (frame)
            {
                frames++;
            }

            // The frame's GPU texture stays valid until this call — see IDesktopVideoCapture.
            capture.ReleaseFrame();
        }
        else
        {
            Thread.Sleep(15);
        }
    }

    Console.WriteLine($"CaptureWindow: {frames} GPU frames in {CaptureSeconds} s.");
}
