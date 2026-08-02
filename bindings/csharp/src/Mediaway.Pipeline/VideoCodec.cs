namespace Mediaway.Pipeline;

/// <summary>
/// Video codec choices accepted by <see cref="AutoVideoEncoder"/>. A narrower type than
/// <see cref="Mediaway.Common.CodecKind"/> (which also lists audio/subtitle codecs) — the
/// native ABI accepts any <c>mediaway_pipeline_codec_kind_t</c> value and only validates at
/// runtime (surfacing a non-video choice as <see cref="MediawayPipelineStatus.Unsupported"/>);
/// this binding restricts the public API to what can actually succeed, at compile time.
/// Numeric values match <see cref="Mediaway.Common.CodecKind"/>'s leading four entries 1:1.
/// </summary>
public enum VideoCodec
{
    H264 = 0,
    Hevc = 1,
    Av1 = 2,
    Vp9 = 3,
}
