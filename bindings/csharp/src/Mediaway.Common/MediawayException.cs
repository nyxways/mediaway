namespace Mediaway.Common;

/// <summary>
/// Base type for every exception a Mediaway C# binding throws. Concrete capability
/// packages (<c>Mediaway.Container</c>, <c>Mediaway.Pipeline</c>, <c>Mediaway.Device</c>)
/// each derive their own leaf type carrying that capability's native status code — never
/// throw this type directly.
/// </summary>
public abstract class MediawayException : Exception
{
    protected MediawayException(string message) : base(message)
    {
    }

    protected MediawayException(string message, Exception innerException) : base(message, innerException)
    {
    }
}
