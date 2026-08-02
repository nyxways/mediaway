namespace Mediaway.Device.Audio;

/// <summary>A source of continuous PCM microphone audio frames — <see cref="Microphone"/>.</summary>
public interface IAudioCapture : IDisposable
{
    /// <summary>
    /// Non-blocking poll for one frame — the low-level primitive both
    /// <see cref="ReadFramesAsync"/> (net8.0) and Unity's synchronous <c>Update()</c> loop
    /// are built on. Returns <see langword="false"/> if nothing was ready yet this call;
    /// <paramref name="frame"/> is <see langword="null"/> in that case. Available on every
    /// target framework — see docs/adr/0018-csharp-netstandard20-unity.md.
    /// </summary>
    bool TryPollFrame(out AudioFrame? frame);

#if NET8_0_OR_GREATER
    /// <summary>
    /// Streams frames as they become available. Backed by a bounded channel: a slow
    /// consumer applies real backpressure instead of frames being silently dropped. A thin
    /// convenience loop over <see cref="TryPollFrame"/> — net8.0 only (ADR-0018).
    /// Callers must fully drain or cancel this before disposing the <see cref="IAudioCapture"/>
    /// — the underlying native handle is thread-confined, and closing it while a poll is
    /// still in flight is a data race.
    /// </summary>
    IAsyncEnumerable<AudioFrame> ReadFramesAsync(CancellationToken cancellationToken = default);
#endif
}
