namespace Mediaway.Container;

/// <summary>
/// C ABI status code returned by fallible <c>mediaway-ffi</c> functions —
/// mirrors <c>mediaway_status_t</c>.
/// </summary>
public enum MediawayContainerStatus
{
    /// <summary>Success.</summary>
    Ok = 0,

    /// <summary>Null pointer, out-of-range index, or mismatched pointer/length pair.</summary>
    InvalidArgument = 1,

    /// <summary>
    /// Typestate violation: adding a track after streaming began, or pushing/flushing/
    /// polling before it began.
    /// </summary>
    InvalidState = 2,

    /// <summary>Invalid or duplicate track id.</summary>
    InvalidTrack = 3,

    /// <summary>Packet does not match a registered track, or has bad framing.</summary>
    InvalidPacket = 4,

    /// <summary>Truncated or malformed ISOBMFF data.</summary>
    InvalidData = 5,

    /// <summary>Reserved for a future native error variant.</summary>
    UnknownError = 6,

    /// <summary>This call caught a Rust panic; the handle is now poisoned.</summary>
    InternalPanic = 7,

    /// <summary>A previous call already poisoned this handle; the call was refused.</summary>
    HandlePoisoned = 8,
}
