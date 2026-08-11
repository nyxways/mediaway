// ScreenRecord.cs — Mediaway C# quick start (Desktop / Screen source).
//
// The `Mediaway.Common`/`Mediaway.Device`/`Mediaway.Device.Desktop`/
// `Mediaway.Device.Audio`/`Mediaway.Device.Hotplug`/`Mediaway.Pipeline`
// packages this targets are real — see bindings/csharp/src/ and
// adr/0004-domain-feature-split.md (mirrored at the C# layer). Screen
// capture is real here (unlike an earlier revision of this file, which
// predated the Desktop package and was design-only) — see CameraRecord.cs
// for the Camera-source counterpart of this same shape.
//
// Unlike Camera, Screen capture is Zero-Copy only: it requires a live
// `ID3D11Device*` the caller owns and keeps alive for the whole session
// (Mediaway.Common.GpuDeviceHandle.DirectX11). The device itself comes from
// Mediaway.Device.GpuDevice.Create — the GPU device factory that closes the
// "no C-ABI caller can build its own D3D11 device" gap (previously this
// example could only demonstrate the gap via a placeholder).
//
// Captured frames are streamed straight into the encoder via
// EncodeSession.WriteFrameFromDesktopCapture — the capture-to-encode bridge
// (adr/pipeline/0005-capture-encode-bridge-c-abi.md): one native call polls
// the capture and pushes the frame, no intermediate frame struct, Zero-Copy
// for Screen's GPU-backed frames. The encoder is opened with
// VideoEncodeConfig.GpuDevice set to the same device the capture uses, so
// the whole capture -> encode path stays Zero-Copy.
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

const int Fps = 30;
const int Seconds = 3;

try
{
    await RecordScreenAsync();
}
catch (CaptureUnavailableException ex)
{
    // Thrown by GpuDevice.Create/DesktopScreenCapture.Open below — an environment
    // problem for this scenario (no D3D11-capable adapter, or no display), not a
    // normal branch, so it is handled once here instead of at the call site. Same
    // exception type CameraRecord.cs catches for Camera.Open — capture-source
    // failures share one type regardless of which source backs them.
    Console.WriteLine($"ScreenRecord: {ex.Message} — exiting.");
}
catch (MediawayPipelineException ex) when (ex.Status is MediawayPipelineStatus.NoBackend or MediawayPipelineStatus.Unsupported)
{
    // NoBackend: thrown by AutoVideoEncoder.Open when no usable H.264 encoder is
    // compiled in — same reasoning as CaptureUnavailableException above, distinct
    // failure domain (encoder vs. capture source), matching
    // EncodeToMp4.cs/CameraRecord.cs's existing convention.
    //
    // Unsupported: this dev machine's WMF/DX11 H.264 encoder backend accepts the
    // GpuDevice-configured open above but rejects the GPU-input encode path itself
    // once frames actually start flowing through WriteFrameFromDesktopCapture — a
    // pre-existing encoder/driver limitation (shared with the Rust
    // gpu_write_frame_smoke.rs test and the C/Node.js screen_record examples), not
    // something the capture-to-encode bridge introduces. Both are graceful,
    // expected outcomes to catch here, not bugs.
    Console.WriteLine($"ScreenRecord: {ex.Message} — exiting.");
}

static async Task RecordScreenAsync()
{
    // ── 1. Open a GPU device + capture sources ───────────────────────────

    // VideoSupport = true (D3D11_CREATE_DEVICE_VIDEO_SUPPORT) — required for the
    // GPU-input encode path this example takes below.
    using var gpuDevice = GpuDevice.Create(new GpuDeviceOptions { VideoSupport = true });

    // Display 0 = primary. The capture settles on its own stream geometry
    // (whatever the display actually is), read back below via Width/Height.
    using var videoCapture = DesktopScreenCapture.Open(
        outputIndex: 0, frameRate: new Rational(1, Fps), gpuDevice: gpuDevice.Handle);

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
        // DXGI Desktop Duplication delivers BGRA8 GPU textures — CreateDefault's NV12 is a
        // CPU-encode assumption, mismatched with what Screen actually captures.
        PixelFormat = PixelFormat.Bgra8,
        // Same device the capture uses — required for the capture-to-encode bridge
        // below to reach the Zero-Copy/GPU-copy encode path instead of
        // MEDIAWAY_PIPELINE_STATUS_UNSUPPORTED.
        GpuDevice = gpuDevice.Handle,
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

// ── Record loop ──────────────────────────────────────────────────────────────
//
// video/session go through EncodeSession.WriteFrameFromDesktopCapture — the
// capture-to-encode bridge (adr/pipeline/0005-capture-encode-bridge-c-abi.md): one
// native call polls `video` and, if a frame was ready, pushes it into the encoder
// and releases it, all inside that one call. No intermediate DesktopVideoFrame
// crosses into managed code at all for the common case, unlike CameraRecord.cs's
// RecordAsync (which still demonstrates the manual TryPollFrame/WriteFrame path).
static async Task RecordAsync(IDesktopVideoCapture video, IAudioCapture? audio, EncodeSession session, CancellationToken token)
{
    Task audioDrainTask = audio is null ? Task.CompletedTask : DrainAudioAsync(audio, token);

    while (!token.IsCancellationRequested)
    {
        if (!session.WriteFrameFromDesktopCapture(video))
        {
            await Task.Delay(TimeSpan.FromMilliseconds(4), token).ConfigureAwait(false);
        }
    }

    await audioDrainTask;

    // Not wired into an audio track yet — this loop only demonstrates that the
    // async frame stream composes the same way for an audio source.
    static async Task DrainAudioAsync(IAudioCapture audio, CancellationToken token)
    {
        // Fully qualified: Mediaway.Pipeline (also `using`d above, for VideoEncodeConfig
        // etc.) has its own AudioFrame type, ambiguous with Mediaway.Device.Audio's.
        await foreach (Mediaway.Device.Audio.AudioFrame frame in audio.ReadFramesAsync(token))
        {
            frame.Dispose(); // AudioFrame also owns a native buffer.
        }
    }
}
