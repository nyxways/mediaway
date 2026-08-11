using Mediaway.Common;
using Mediaway.Device;
using Mediaway.Device.Desktop;
using Xunit;
using Xunit.Abstractions;

namespace Mediaway.Pipeline.Tests;

/// <summary>
/// Exercises the real capture-to-encode bridge end to end
/// (adr/pipeline/0005-capture-encode-bridge-c-abi.md) — mirrors
/// crates/mediaway-ffi/tests/screen_capture_encode_bridge_smoke.rs and the C/Node.js
/// <c>screen_record</c> examples: factory-created GPU device (adr/0007-gpu-device-factory.md)
/// -&gt; real Screen capture opened with it -&gt; <see cref="EncodeSession.WriteFrameFromDesktopCapture"/>,
/// no intermediate frame struct crossing into managed code. Unlike
/// <c>EncodeToMp4Tests</c> (WMF/DX11 H.264 encode alone, always expected to work on this
/// machine and in CI), every step here has an environment-dependent failure mode with no
/// good way to force it deterministically (no GPU/video-support adapter, DXGI Desktop
/// Duplication unavailable in a headless/RDP CI session, GPU-input encode itself
/// unsupported) — so, like the Rust smoke test it mirrors, this soft-skips (logs and
/// returns) at each stage instead of failing the run, and only asserts real work once
/// frames actually made it all the way to a finished MP4.
/// </summary>
public sealed class ScreenCaptureEncodeBridgeTests
{
    private readonly ITestOutputHelper _output;

    public ScreenCaptureEncodeBridgeTests(ITestOutputHelper output) => _output = output;

    [Fact]
    public void WriteFrameFromDesktopCapture_BridgesRealScreenFramesIntoTheEncoder()
    {
        GpuDevice? gpuDevice;
        try
        {
            gpuDevice = GpuDevice.Create(new GpuDeviceOptions { VideoSupport = true });
        }
        catch (MediawayDeviceException ex)
        {
            _output.WriteLine($"skip: GpuDevice.Create failed ({ex.Status}) — {ex.Message}");
            return;
        }

        using (gpuDevice)
        {
            IDesktopVideoCapture? capture;
            try
            {
                capture = DesktopScreenCapture.Open(
                    outputIndex: 0, frameRate: new Rational(1, 30), gpuDevice: gpuDevice.Handle);
            }
            catch (MediawayDeviceException ex)
            {
                _output.WriteLine($"skip: screen capture open failed ({ex.Status}) — DDA unavailable? {ex.Message}");
                return;
            }

            using (capture)
            {
                Assert.True(capture.Width > 0, "Screen capture did not negotiate a real width.");
                Assert.True(capture.Height > 0, "Screen capture did not negotiate a real height.");

                var config = VideoEncodeConfig.CreateDefault(
                    VideoCodec.H264, capture.Width, capture.Height, new Rational(1, 30)) with
                {
                    BitrateBps = 4_000_000,
                    // DXGI Desktop Duplication delivers BGRA8 GPU textures — CreateDefault's
                    // NV12 is a CPU-encode assumption, mismatched with what Screen actually
                    // captures (same fix ScreenRecord.cs and the Rust smoke test need).
                    PixelFormat = PixelFormat.Bgra8,
                    // Same device the capture uses — required to reach the Zero-Copy/GPU-copy
                    // encode path this bridge relies on.
                    GpuDevice = gpuDevice.Handle,
                };

                AutoVideoEncoder encoder;
                try
                {
                    encoder = AutoVideoEncoder.Open(config);
                }
                catch (MediawayPipelineException ex) when (
                    ex.Status is MediawayPipelineStatus.NoBackend or MediawayPipelineStatus.Unsupported)
                {
                    _output.WriteLine($"skip: video encoder unavailable for GPU input ({ex.Status}) — {ex.Message}");
                    return;
                }

                using var session = EncodeSession.Open(encoder);

                var deadline = DateTime.UtcNow + TimeSpan.FromSeconds(3);
                int written = 0;
                try
                {
                    while (DateTime.UtcNow < deadline && written < 3)
                    {
                        if (session.WriteFrameFromDesktopCapture(capture))
                        {
                            written++;
                        }
                        else
                        {
                            Thread.Sleep(15);
                        }
                    }
                }
                catch (MediawayPipelineException ex) when (ex.Status is MediawayPipelineStatus.Unsupported)
                {
                    // Same known dev-machine limitation as EncodeToMp4Tests can hit — a backend
                    // that accepts a GpuDevice-configured open but rejects the GPU-input encode
                    // path once frames actually start flowing
                    // (adr/pipeline/0002-gpu-frame-input-c-abi.md).
                    _output.WriteLine($"skip: GPU-input encode unsupported on this backend ({ex.Message})");
                    return;
                }

                if (written == 0)
                {
                    _output.WriteLine("skip: screen capture opened but delivered no frames within 3 s.");
                    return;
                }

                using var mp4Bytes = session.Finish();
                Assert.True(mp4Bytes.Memory.Length > 0, "Encoder produced no bytes after Finish().");
                _output.WriteLine($"Bridged {written} real screen frame(s) into {mp4Bytes.Memory.Length} MP4 byte(s).");
            }
        }
    }
}
