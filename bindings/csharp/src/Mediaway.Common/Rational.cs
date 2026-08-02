namespace Mediaway.Common;

/// <summary>
/// A rational timebase (<c>Num / Den</c>, seconds) — mirrors the native
/// <c>mediaway_rational_t</c> shared across every <c>mediaway-*-ffi</c> C ABI.
/// </summary>
/// <param name="Num">Numerator (timestamp units).</param>
/// <param name="Den">Denominator (timebase / timescale). Must be non-zero.</param>
public readonly record struct Rational(ulong Num, uint Den);
