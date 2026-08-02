using System.Runtime.CompilerServices;

// MediawayDeviceException.ThrowIfError (internal) is called from every leaf package's
// Interop layer — each leaf package gets its own P/Invoke declarations and native structs
// (adr/0004-domain-feature-split.md mirrored at the C# layer), but they all share this
// base package's status→exception mapping rather than duplicating it four times.
[assembly: InternalsVisibleTo("Mediaway.Device.Camera")]
[assembly: InternalsVisibleTo("Mediaway.Device.Desktop")]
[assembly: InternalsVisibleTo("Mediaway.Device.Audio")]
[assembly: InternalsVisibleTo("Mediaway.Device.Hotplug")]
