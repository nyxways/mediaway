namespace Mediaway.Device.Desktop;

/// <summary>
/// A source of continuous PCM audio frames capturing what the desktop is already
/// rendering (Loopback / ProcessLoopback) — <see cref="DesktopAudioCapture"/>. Named
/// distinctly from <c>Mediaway.Device.Audio.IAudioCapture</c> (not shared/reused) so a
/// consumer referencing both packages together (a very plausible combo — mic + desktop
/// loopback in the same app) never hits an ambiguous-reference error on a bare
/// <c>AudioFrame</c>/<c>IAudioCapture</c> name.
/// </summary>
public interface IDesktopAudioCapture : IDisposable
{
    /// <summary>
    /// Non-blocking poll for one frame — the low-level primitive both
    /// <see cref="ReadFramesAsync"/> (net8.0) and Unity's synchronous <c>Update()</c> loop
    /// are built on. Returns <see langword="false"/> if nothing was ready yet this call;
    /// <paramref name="frame"/> is <see langword="null"/> in that case. Available on every
    /// target framework — see docs/adr/0018-csharp-netstandard20-unity.md.
    /// </summary>
    bool TryPollFrame(out DesktopAudioFrame? frame);

#if NET8_0_OR_GREATER
    /// <summary>
    /// Streams frames as they become available. Backed by a bounded channel: a slow
    /// consumer applies real backpressure instead of frames being silently dropped. A thin
    /// convenience loop over <see cref="TryPollFrame"/> — net8.0 only (ADR-0018).
    /// Callers must fully drain or cancel this before disposing the
    /// <see cref="IDesktopAudioCapture"/> — the underlying native handle is
    /// thread-confined, and closing it while a poll is still in flight is a data race.
    /// </summary>
    IAsyncEnumerable<DesktopAudioFrame> ReadFramesAsync(CancellationToken cancellationToken = default);
#endif
}
