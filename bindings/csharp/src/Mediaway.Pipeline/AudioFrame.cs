namespace Mediaway.Pipeline;

/// <summary>
/// Input to <see cref="AudioEncoder.PushPcm"/> — <see cref="Data"/> is borrowed for the call
/// only. Distinct from <c>Mediaway.Device.Audio.AudioFrame</c> (a capture OUTPUT that owns a
/// disposable native buffer) the same way <see cref="VideoFrame"/> is distinct from
/// <c>Mediaway.Device.Camera.VideoFrame</c> — see CameraRecord.cs's own revision note.
/// </summary>
public sealed record AudioFrame
{
    public required long Pts { get; init; }

    /// <summary><c>0</c> if unknown.</summary>
    public required ulong Duration { get; init; }

    public required uint SampleRate { get; init; }

    public required ushort Channels { get; init; }

    /// <summary>Interleaved F32 PCM — the only sample format the real backend accepts today.</summary>
    public required ReadOnlyMemory<byte> Data { get; init; }
}
