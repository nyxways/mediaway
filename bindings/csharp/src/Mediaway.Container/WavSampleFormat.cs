namespace Mediaway.Container;

/// <summary>
/// RIFF/WAVE <c>fmt</c> chunk sample encoding (<c>wFormatTag</c>) — mirrors
/// <c>mediaway_wav_sample_format_t</c>. NOT the same enum as <c>Mediaway.Common.SampleFormat</c>
/// (raw PCM bit depth for device/pipeline audio, S16/S32/F32) — this is the WAVE container's
/// own tag encoding.
/// </summary>
public enum WavSampleFormat
{
    /// <summary>Integer PCM (<c>wFormatTag</c> 1).</summary>
    Pcm = 0,

    /// <summary>IEEE float PCM (<c>wFormatTag</c> 3).</summary>
    Float = 1,
}
