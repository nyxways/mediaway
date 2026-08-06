using Mediaway.Common;

namespace Mediaway.Pipeline;

/// <summary>
/// Config for <see cref="AutoVideoEncoder.Open"/>. A plain value type — no native handle,
/// nothing to dispose.
/// </summary>
public sealed record VideoEncodeConfig
{
    public required VideoCodec Codec { get; init; }

    public required uint Width { get; init; }

    public required uint Height { get; init; }

    public required Rational TimeBase { get; init; }

    /// <summary><c>0</c> means "let the backend pick its own default".</summary>
    public uint BitrateBps { get; init; }

    public PixelFormat PixelFormat { get; init; } = PixelFormat.Nv12;

    /// <summary>
    /// GPU device to open the encoder against. <see cref="GpuDeviceHandle.None"/> (the
    /// default) keeps the existing CPU-only path; a real device (e.g.
    /// <see cref="GpuDeviceHandle.DirectX11"/>) opts into the Zero-Copy/GPU-copy input
    /// path used by <see cref="EncodeSession.WriteGpuFrame"/>. Caller-owned — must
    /// outlive the <see cref="AutoVideoEncoder.Open"/> call.
    /// </summary>
    public GpuDeviceHandle GpuDevice { get; init; } = GpuDeviceHandle.None;

    /// <summary>
    /// Frames between forced IDR refreshes. <c>1</c> (default) is IDR-only —
    /// byte-identical to today's behavior for every existing caller.
    /// <para>
    /// <b>Not yet honored by the auto-selected backend on any platform this package
    /// opens</b> (native ABI v6, <c>adr/0001-auto-encode-c-abi.md</c>'s 2026-08-07
    /// addendum) — only the standalone Vulkan H.264/HEVC Rust encoders read this
    /// today, and <see cref="AutoVideoEncoder"/> cannot reach them yet. Setting this
    /// is a forward-compatible no-op for now: the auto-selected encoder (WMF here)
    /// ignores it and keeps producing IDR-only output, with no error.
    /// </para>
    /// </summary>
    public uint GopSize { get; init; } = 1;

    /// <summary>
    /// CBR-style rate control request. <c>null</c> (default) keeps fixed-QP encoding.
    /// Same <b>not yet honored via auto-select</b> caveat as <see cref="GopSize"/>
    /// applies here too.
    /// </summary>
    public RateControlConfig? RateControl { get; init; }

    /// <summary>
    /// Explicit size and codec — resolution always comes from the caller, never a named
    /// preset. Defaults <see cref="BitrateBps"/> to <c>0</c> (backend default),
    /// <see cref="PixelFormat"/> to NV12, and <see cref="GpuDevice"/> to
    /// <see cref="GpuDeviceHandle.None"/> (CPU-only), matching the native
    /// <c>mediaway_auto_video_encode_config_new</c>'s own defaults.
    /// </summary>
    public static VideoEncodeConfig CreateDefault(VideoCodec codec, uint width, uint height, Rational timeBase) =>
        new()
        {
            Codec = codec,
            Width = width,
            Height = height,
            TimeBase = timeBase,
        };
}
