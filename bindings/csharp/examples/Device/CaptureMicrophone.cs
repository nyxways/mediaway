// CaptureMicrophone.cs — Mediaway C# quick start (microphone-only capture, raw PCM).
//
// The `Mediaway.Common`/`Mediaway.Device`/`Mediaway.Device.Audio` packages this targets are
// real — see bindings/csharp/src/ and adr/0004-domain-feature-split.md. Mirrors
// examples/device/capture_microphone.rs (and bindings/nodejs's own capture-microphone.ts):
// open the default mic, poll ~2 s of PCM frames, print the negotiated format, close. No
// encoding here — see Pipeline/EncodeAudio.cs for the AAC-encode counterpart of this same
// capture.
//
// Run:
//     dotnet run

using Mediaway.Common;
using Mediaway.Device.Audio;

const int RecordSeconds = 2;

IAudioCapture? mic = Microphone.TryOpen(sampleRateTimeBase: new Rational(1, 48_000), out var error);
if (mic is null)
{
    Console.WriteLine($"CaptureMicrophone: no microphone on this machine ({error}) — nothing to capture.");
    return;
}

using (mic)
{
    int frames = 0;
    long totalBytes = 0;
    using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(RecordSeconds));
    try
    {
        await foreach (AudioFrame frame in mic.ReadFramesAsync(cts.Token))
        {
            using (frame)
            {
                if (frames == 0)
                {
                    Console.WriteLine(
                        $"CaptureMicrophone: mic negotiated {frame.SampleRate} Hz, {frame.Channels} ch, {frame.SampleFormat}");
                }

                frames++;
                totalBytes += frame.Data.Length;
            }
        }
    }
    catch (OperationCanceledException)
    {
        // Expected: the recording duration elapsed.
    }

    Console.WriteLine($"CaptureMicrophone: captured {frames} PCM frame(s), {totalBytes} bytes in ~{RecordSeconds}s.");
}
