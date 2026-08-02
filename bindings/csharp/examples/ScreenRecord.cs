// ScreenRecord.cs — Mediaway C# quick start (Desktop / Screen source).
//
// The `Mediaway.Common`/`Mediaway.Device.Desktop`/`Mediaway.Device.Audio`/
// `Mediaway.Device.Hotplug`/`Mediaway.Pipeline` packages this targets are
// real — see bindings/csharp/src/ and adr/0004-domain-feature-split.md
// (mirrored at the C# layer). Screen capture is real here (unlike an earlier
// revision of this file, which predated the Desktop package and was
// design-only) — see CameraRecord.cs for the Camera-source counterpart of
// this same shape.
//
// Unlike Camera, Screen capture is Zero-Copy only: it requires a live
// `ID3D11Device*` the caller owns and keeps alive for the whole session
// (Mediaway.Device.Desktop.GpuDeviceHandle.DirectX11), and polled frames'
// GPU texture handle (Mediaway.Device.Desktop.DesktopVideoFrame.GpuBuffer)
// must be explicitly released via IDesktopVideoCapture.ReleaseFrame() before
// the next poll — see that interface's own doc comment for why this
// binding does not auto-release Screen frames the way Camera's does.
// Creating the D3D11 device itself is out of scope for this example (it
// needs either raw COM interop or a Direct3D wrapper package like
// Vortice.Direct3D11 — a real, separate choice for whatever app embeds
// this); `OpenSharedD3D11Device` below is a placeholder for wherever the
// caller's own device comes from.
//
// The real Screen backend is Gpu-storage only (no CPU fallback) — every polled
// frame is fed straight into Mediaway.Pipeline.EncodeSession.WriteGpuFrame, the
// GPU-backed sibling of WriteFrame (adr/0002-gpu-frame-input-c-abi.md in the Rust
// crate), no CPU readback involved. The encoder is opened with
// VideoEncodeConfig.GpuDevice set to the same D3D11 device the capture uses, so the
// whole capture -> encode path stays Zero-Copy.
//
// Scenario: capture the primary display + default microphone, encode the video
// into H.264 through the same "auto video encoder -> encode session" building
// blocks as EncodeToMp4.cs, and write a fragmented MP4 to disk. Also watches the
// microphone for hotplug changes during the recording (e.g. a USB headset
// unplugged mid-session) via the real native push-mode callback and logs them —
// recording continues without audio rather than failing, mirroring
// CameraRecord.cs's "mic unavailable -> degrade, don't exit" stance.
//
// Run:
//     dotnet run

using System.Buffers;
using Mediaway.Common;
using Mediaway.Device;
using Mediaway.Device.Audio;
using Mediaway.Device.Desktop;
using Mediaway.Device.Hotplug;
using Mediaway.Pipeline;
using DesktopVideoFrame = Mediaway.Device.Desktop.DesktopVideoFrame;
using PipelineVideoFrame = Mediaway.Pipeline.VideoFrame;

const int Fps = 30;
const int Seconds = 3;

try
{
    await RecordScreenAsync();
}
catch (CaptureUnavailableException ex)
{
    // Thrown by DesktopScreenCapture.Open below — an environment problem for this
    // scenario (no display, or no D3D11-capable device), not a normal branch, so
    // it is handled once here instead of at the call site. Same exception type
    // CameraRecord.cs catches for Camera.Open — capture-source failures share one
    // type regardless of which source backs them.
    Console.WriteLine($"ScreenRecord: {ex.Message} — exiting.");
}
catch (EncoderUnavailableException ex)
{
    // Thrown by AutoVideoEncoder.Open below — same reasoning, distinct type
    // from CaptureUnavailableException because it's a different failure
    // domain (no usable H.264 encoder vs. no capture source), matching
    // EncodeToMp4.cs/CameraRecord.cs's existing convention.
    Console.WriteLine($"ScreenRecord: {ex.Message} — exiting.");
}

static async Task RecordScreenAsync()
{
    // ── 1. Open a D3D11 device + capture sources ─────────────────────────

    nint d3d11Device = OpenSharedD3D11Device();

    // Display 0 = primary. The capture settles on its own stream geometry
    // (whatever the display actually is), read back below via Width/Height.
    using var videoCapture = DesktopScreenCapture.Open(
        outputIndex: 0, frameRate: new Rational(1, Fps), gpuDevice: GpuDeviceHandle.DirectX11(d3d11Device));

    // Recording without audio is a valid degraded mode — log and carry on
    // rather than exiting. `using` on a null IAudioCapture? is a documented
    // no-op.
    using var audioCapture = Microphone.TryOpen(sampleRateTimeBase: new Rational(1, 48_000), out var audioError);
    if (audioCapture is null)
    {
        Console.WriteLine($"ScreenRecord: microphone unavailable ({audioError}) — continuing without audio.");
    }

    uint width = videoCapture.Width;
    uint height = videoCapture.Height;
    Console.WriteLine($"ScreenRecord: {width}x{height} display, mic {(audioCapture is null ? "unavailable" : "ready")}");

    // Watch the microphone for hotplug changes while recording, via the real
    // native push-mode callback (mediaway_device_hotplug_register_callback) —
    // subscribing to DeviceChanged registers it; no polling loop of our own.
    // Unlike the opens above, this Open() throws for a different reason:
    // registering the OS notification callback failing is a genuine COM-level
    // error, not an "expected absence" TryOpen exists for. Only watched when a
    // microphone is actually in use.
    using var hotplug = audioCapture is null ? null : DeviceHotplug.Open(DeviceKind.Microphone);
    if (hotplug is not null)
    {
        hotplug.DeviceChanged += (_, e) =>
            Console.WriteLine($"ScreenRecord: {e.Kind} device {e.ChangeType} ({e.DeviceId})");
    }

    // ── 2. Open the encoder + encode session (same building blocks as EncodeToMp4.cs) ──

    var config = VideoEncodeConfig.CreateDefault(VideoCodec.H264, width, height, new Rational(1, Fps)) with
    {
        BitrateBps = 8_000_000,
        // Same D3D11 device the capture uses — required for WriteGpuFrame below to
        // reach the Zero-Copy/GPU-copy encode path instead of MEDIAWAY_PIPELINE_STATUS_UNSUPPORTED.
        GpuDevice = GpuDeviceHandle.DirectX11(d3d11Device),
    };

    using var encoder = AutoVideoEncoder.Open(config);
    using var session = EncodeSession.Open(encoder);

    // ── 3. Stream captured frames straight into the encoder ─────────────

    using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(Seconds));
    try
    {
        await RecordAsync(videoCapture, audioCapture, session, cts.Token);
    }
    catch (OperationCanceledException)
    {
        // Expected: the recording duration elapsed.
    }

    // ── 4. Finish encoding and write the result ──────────────────────────

    using IMemoryOwner<byte> mp4Bytes = session.Finish();
    await using (var outFile = File.Create("out_screen.mp4"))
    {
        await outFile.WriteAsync(mp4Bytes.Memory);
    }

    Console.WriteLine($"ScreenRecord: {width}x{height} -> out_screen.mp4 ({mp4Bytes.Memory.Length} bytes)");
}

/// <summary>
/// Placeholder: a real app supplies its own live <c>ID3D11Device*</c> here (e.g. via
/// Vortice.Direct3D11's <c>D3D11.D3D11CreateDevice</c>, or raw COM interop) and keeps it
/// alive for the whole capture session — <see cref="GpuDeviceHandle"/> only borrows it.
/// Building that device is a Direct3D interop concern this binding deliberately does not
/// wrap (see <see cref="GpuDeviceHandle"/>'s own docs).
/// </summary>
static nint OpenSharedD3D11Device() =>
    throw new NotImplementedException(
        "Supply a live ID3D11Device* here — see this method's doc comment.");

// ── Record loop ──────────────────────────────────────────────────────────────
//
// Every parameter here is an interface type (or the session built on one) — this
// method has no idea which concrete OS backend is underneath. Unlike Camera's
// RecordAsync (CameraRecord.cs), this polls synchronously instead of using
// ReadFramesAsync — IDesktopVideoCapture deliberately has no async wrapper (see its
// own doc comment: buffering GPU-backed frames in a channel is not safe here), and
// every polled frame's GpuBuffer must be released via ReleaseFrame() before the next
// poll that acquires again.
static async Task RecordAsync(IDesktopVideoCapture video, IAudioCapture? audio, EncodeSession session, CancellationToken token)
{
    Task audioDrainTask = audio is null ? Task.CompletedTask : DrainAudioAsync(audio, token);

    long pts = 0;
    while (!token.IsCancellationRequested)
    {
        if (video.TryPollFrame(out DesktopVideoFrame? frame))
        {
            using (frame)
            {
                if (frame!.StorageKind == VideoFrameStorageKind.Gpu)
                {
                    // The real Screen backend today — Zero-Copy straight into the encoder,
                    // no CPU readback. frame.GpuBuffer stays valid (and un-released) until
                    // ReleaseFrame() below, which is only called after WriteGpuFrame's
                    // synchronous call returns.
                    session.WriteGpuFrame(new GpuVideoFrame
                    {
                        Pts = pts++,
                        Duration = 1,
                        Width = frame.Width,
                        Height = frame.Height,
                        PixelFormat = frame.PixelFormat,
                        GpuBuffer = frame.GpuBuffer,
                    });
                }
                else
                {
                    session.WriteFrame(new PipelineVideoFrame
                    {
                        Pts = pts++,
                        Duration = 1,
                        Width = frame.Width,
                        Height = frame.Height,
                        PixelFormat = frame.PixelFormat,
                        Data = frame.Data,
                    });
                }
            }

            video.ReleaseFrame(); // Invalidates frame.GpuBuffer — call only after done using it.
        }
        else
        {
            await Task.Delay(TimeSpan.FromMilliseconds(4), token).ConfigureAwait(false);
        }
    }

    await audioDrainTask;

    // Not wired into an audio track yet — this loop only demonstrates that the
    // async frame stream composes the same way for an audio source.
    static async Task DrainAudioAsync(IAudioCapture audio, CancellationToken token)
    {
        await foreach (AudioFrame frame in audio.ReadFramesAsync(token))
        {
            frame.Dispose(); // AudioFrame also owns a native buffer.
        }
    }
}
