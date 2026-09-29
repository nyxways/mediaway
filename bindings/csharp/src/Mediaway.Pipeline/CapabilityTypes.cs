namespace Mediaway.Pipeline;

/// <summary>Which encode backend a capability row describes. Mirrors <c>mediaway_encode_backend_t</c>.</summary>
public enum EncodeBackend
{
    /// <summary>The platform's own media API (Media Foundation, VideoToolbox, VA-API).</summary>
    Os = 0,

    /// <summary>NVIDIA NVENC.</summary>
    Nvenc = 1,

    /// <summary>Intel Quick Sync Video.</summary>
    QuickSync = 2,

    /// <summary>AMD AMF.</summary>
    Amf = 3,

    /// <summary><c>VK_KHR_video_encode_queue</c>.</summary>
    Vulkan = 4,

    /// <summary>Pure-Rust software encoder.</summary>
    Software = 5,

    /// <summary>A backend added after this binding's ABI version.</summary>
    Unknown = 255,
}

/// <summary>Whether a backend or codec is usable right now. Mirrors <c>mediaway_support_state_t</c>.</summary>
public enum SupportState
{
    /// <summary>Usable; for an encoder row, <see cref="EncoderCapability.PathClass"/> says at what cost.</summary>
    Supported = 0,

    /// <summary>No code path exists for this combination on this platform.</summary>
    NotImplemented = 1,

    /// <summary>Real code exists, but the driver or device did not answer right now.</summary>
    NoDevice = 2,

    /// <summary>A state added after this binding's ABI version.</summary>
    Unknown = 255,
}

/// <summary>The cheapest data path a supported encoder reached. Mirrors <c>mediaway_encode_path_class_t</c>.</summary>
public enum EncodePathClass
{
    /// <summary>The row is not <see cref="SupportState.Supported"/>.</summary>
    None = 0,

    /// <summary>GPU handle accepted with no copy or readback.</summary>
    ZeroCopy = 1,

    /// <summary>GPU-to-GPU copy or cross-API share; no CPU round trip.</summary>
    GpuCopy = 2,

    /// <summary>CPU planes uploaded into a hardware encoder.</summary>
    CpuUpload = 3,

    /// <summary>GPU-to-CPU readback, then encode (costly).</summary>
    Readback = 4,

    /// <summary>Software encoder.</summary>
    Software = 5,

    /// <summary>A path class added after this binding's ABI version.</summary>
    Unknown = 255,
}

/// <summary>One row of <see cref="EncoderSupport.Query(Mediaway.Common.CodecKind, uint, uint)"/>.</summary>
public sealed record EncoderCapability
{
    /// <summary>Which backend this row describes.</summary>
    public required EncodeBackend Backend { get; init; }

    /// <summary>Whether it is usable right now.</summary>
    public required SupportState State { get; init; }

    /// <summary>Data-path cost; <see cref="EncodePathClass.None"/> unless <see cref="State"/> is <see cref="SupportState.Supported"/>.</summary>
    public required EncodePathClass PathClass { get; init; }
}
