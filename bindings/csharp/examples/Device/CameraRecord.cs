// CameraRecord.cs — Mediaway C# quick start.
//
// The `Mediaway.Common`/`Mediaway.Device.Camera`/`Mediaway.Device.Audio`/
// `Mediaway.Pipeline` packages this targets are real — see bindings/csharp/src/
// and adr/0004-domain-feature-split.md (mirrored at the C# layer: Camera,
// Desktop, Audio, and Hotplug are now separate packages, not one
// `Mediaway.Device`). See ScreenRecord.cs for the Desktop (Screen) source
// counterpart of this same shape.
//
// Revision note (matching the real, shipped API): each package keeps its
// own root namespace. `IVideoCapture.Width`/`Height` are `uint`, matching
// the native ABI's `uint32_t` geometry. `Mediaway.Device.Camera.VideoFrame`
// and `Mediaway.Pipeline.VideoFrame` are two DISTINCT types, not one shared
// record: the Camera frame owns a disposable native buffer (Zero-Copy poll
// output — see Mediaway.Common.Interop.NativeOwnedMemoryManager), while the
// Pipeline frame is a plain, non-disposable borrowed-input value type. So
// RecordAsync below explicitly builds a new Pipeline frame from each
// captured Camera frame's fields and disposes the Camera frame afterward,
// instead of a single-type `with` clone. `EncodeSession.Finish()` returns an
// `IMemoryOwner<byte>` for the same reason (see EncodeToMp4.cs's own
// revision note).
//
// Scenario: capture the default webcam + default microphone, encode the
// video into H.264 through the same "auto video encoder -> encode session"
// building blocks as EncodeToMp4.cs, and write a fragmented MP4 to disk.
//
// Run:
//     dotnet run

using System.Buffers;
using Mediaway.Common;
using Mediaway.Device;
using Mediaway.Device.Audio;
using Mediaway.Device.Camera;
using Mediaway.Pipeline;
using DeviceVideoFrame = Mediaway.Device.Camera.VideoFrame;
using PipelineVideoFrame = Mediaway.Pipeline.VideoFrame;

const int Fps = 30;
const int Seconds = 3;

try
{
    await RunRecordingAsync();
}
catch (CaptureUnavailableException ex)
{
    // Thrown by Camera.Open below — no webcam is an environment problem for
    // this scenario, not a normal branch (unlike a missing microphone,
    // handled at its own call site via TryOpen below).
    Console.WriteLine($"CameraRecord: {ex.Message} — exiting.");
}
catch (EncoderUnavailableException ex)
{
    // Thrown by AutoVideoEncoder.Open below.
    Console.WriteLine($"CameraRecord: {ex.Message} — exiting.");
}

static async Task RunRecordingAsync()
{
    // ── 1. Open capture sources ─────────────────────────────────────────

    // Device 0 = default/first camera. The capture settles on its own
    // stream geometry (whatever the camera actually negotiated), read back
    // below via Width/Height. No camera -> nothing to record -> Open()
    // (throws), caught above.
    using var videoCapture = Camera.Open(deviceIndex: 0, frameRate: new Rational(1, Fps));

    // Recording without audio is a valid degraded mode — log and carry on
    // rather than exiting. `using` on a null IAudioCapture? is a documented
    // no-op.
    using var audioCapture = Microphone.TryOpen(sampleRateTimeBase: new Rational(1, 48_000), out var audioError);
    if (audioCapture is null)
    {
        Console.WriteLine($"CameraRecord: microphone unavailable ({audioError}) — continuing without audio.");
    }

    uint width = videoCapture.Width;
    uint height = videoCapture.Height;
    Console.WriteLine($"CameraRecord: {width}x{height} camera, mic {(audioCapture is null ? "unavailable" : "ready")}");

    // ── 2. Open the encoder + encode session (same building blocks as EncodeToMp4.cs) ──

    var config = VideoEncodeConfig.CreateDefault(VideoCodec.H264, width, height, new Rational(1, Fps)) with
    {
        BitrateBps = 4_000_000,
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
    await using (var outFile = File.Create("out_camera.mp4"))
    {
        await outFile.WriteAsync(mp4Bytes.Memory);
    }

    Console.WriteLine($"CameraRecord: {width}x{height} -> out_camera.mp4 ({mp4Bytes.Memory.Length} bytes)");
}

// ── Record loop ──────────────────────────────────────────────────────────────
//
// Every parameter here is an interface type (or the session built on one) — this
// method has no idea which concrete OS backend is underneath, so it works
// identically whether `video`/`audio` are camera+mic capture, screen+mic
// capture, or test doubles. Identical shape to ScreenRecord.cs's RecordAsync —
// duplicated rather than shared, matching this binding set's convention that
// each example file stays self-contained and copy-pastable on its own.
static async Task RecordAsync(IVideoCapture video, IAudioCapture? audio, EncodeSession session, CancellationToken token)
{
    Task audioDrainTask = audio is null ? Task.CompletedTask : DrainAudioAsync(audio, token);

    long pts = 0;
    await foreach (DeviceVideoFrame frame in video.ReadFramesAsync(token))
    {
        using (frame) // Device.VideoFrame owns a native buffer — dispose after this frame is written.
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
