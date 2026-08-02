using System.Runtime.InteropServices;
using Mediaway.Device;
using Mediaway.Device.Hotplug.Interop;

namespace Mediaway.Device.Hotplug;

/// <summary>
/// Watches for device add/remove/default-change/state-change notifications for the given
/// <see cref="DeviceKind"/> set. Two mutually exclusive modes on one handle (native
/// enforces this — mixing throws <see cref="MediawayDeviceException"/> with
/// <see cref="MediawayDeviceStatus.CallbackModeActive"/>): pull via <see cref="PollEvent"/>,
/// or push via the real native callback behind <see cref="DeviceChanged"/>
/// (<c>mediaway_device_hotplug_register_callback</c>,
/// <c>adr/0002-callback-event-delivery.md</c>) — subscribing/unsubscribing the last handler
/// transparently registers/unregisters the native callback.
/// </summary>
/// <remarks>
/// <b>Native callback thread-safety contract</b> (inherited verbatim from the C header —
/// this binding cannot soften it, only surface it):
/// <list type="bullet">
/// <item><see cref="DeviceChanged"/> handlers run on an unspecified, Mediaway-owned thread
/// — never the thread that subscribed, and not necessarily the platform backend's own raw
/// OS-callback thread. Do not assume any particular thread identity or COM apartment
/// state.</item>
/// <item>Handlers <b>must not block</b>: the native side runs one bridging thread per
/// handle: blocking it delays every subsequent event and blocks
/// <see cref="Dispose"/>/the last <c>-=</c> for as long as it blocks.</item>
/// <item>Handlers <b>must not call back into any member of this same
/// <see cref="DeviceHotplug"/> instance synchronously</b> (including <see cref="Dispose"/>
/// or removing the last subscription) — the native header documents this as a real,
/// reproduced deadlock, not a theoretical concern.</item>
/// <item>An exception thrown from a handler is caught and swallowed by this binding's
/// native-facing trampoline, never rethrown to the caller — an unhandled exception
/// escaping the unmanaged→managed boundary here would terminate the process
/// (<see cref="System.Runtime.InteropServices.UnmanagedCallersOnlyAttribute"/>'s own
/// documented behavior), the C# analog of the native contract "must not unwind across the
/// FFI boundary." There is no error-reporting channel back through the native callback
/// either way — a handler that can fail should catch its own exceptions.</item>
/// </list>
/// </remarks>
public sealed class DeviceHotplug : IDisposable
{
    private readonly HotplugHandle _handle;
    private readonly object _gate = new();
    private EventHandler<DeviceChangedEventArgs>? _deviceChanged;
    private GCHandle _selfHandle;
    private bool _callbackRegistered;
    private bool _disposed;

#if !NET8_0_OR_GREATER
    /// <summary>
    /// A single, <c>static readonly</c> delegate wrapping a static method with no captured
    /// state — its classic-marshalling thunk stays valid for the whole process lifetime,
    /// not just one registration, so no per-instance delegate needs to be kept alive.
    /// </summary>
    private static readonly unsafe NativeHotplugCallback CallbackDelegate = NativeCallback;
#endif

    private DeviceHotplug(HotplugHandle handle) => _handle = handle;

    /// <param name="kinds">The device kinds to watch (e.g. <see cref="DeviceKind.Microphone"/>).</param>
    public static unsafe DeviceHotplug Open(params DeviceKind[] kinds)
    {
        fixed (DeviceKind* ptr = kinds)
        {
            var status = NativeMethods.mediaway_device_hotplug_open(ptr, (nuint)kinds.Length, out nint handle);
            MediawayDeviceException.ThrowIfError(status);
            return new DeviceHotplug(HotplugHandle.Wrap(handle));
        }
    }

    /// <summary>
    /// Pull the next hotplug event, if any is ready yet. Only valid while no
    /// <see cref="DeviceChanged"/> handler is registered — see <see cref="MediawayDeviceStatus.CallbackModeActive"/>.
    /// </summary>
    public DeviceChangedEventArgs? PollEvent()
    {
        var status = NativeMethods.mediaway_device_hotplug_poll_event(_handle, out var native, out byte hasEvent);
        MediawayDeviceException.ThrowIfError(status);
        if (hasEvent == 0)
        {
            return null;
        }

        var args = ToManaged(native);
        NativeMethods.mediaway_device_hotplug_event_free(ref native);
        return args;
    }

    /// <summary>
    /// Push-mode notifications, delivered through the real native callback
    /// (<c>mediaway_device_hotplug_register_callback</c>) — not a polling loop this binding
    /// runs internally. The first subscription registers the native callback; the callback
    /// is unregistered again once the last handler is removed. See this type's own
    /// <see cref="DeviceHotplug"/> remarks for the thread-safety contract every handler must
    /// honor.
    /// </summary>
    public event EventHandler<DeviceChangedEventArgs>? DeviceChanged
    {
        add
        {
            lock (_gate)
            {
                bool wasEmpty = _deviceChanged is null;
                _deviceChanged += value;
                if (wasEmpty && !_disposed)
                {
                    RegisterCallback();
                }
            }
        }
        remove
        {
            lock (_gate)
            {
                _deviceChanged -= value;
                if (_deviceChanged is null && _callbackRegistered)
                {
                    UnregisterCallback();
                }
            }
        }
    }

    /// <summary>Caller must already hold <see cref="_gate"/>.</summary>
    private unsafe void RegisterCallback()
    {
        _selfHandle = GCHandle.Alloc(this, GCHandleType.Normal);
        nint userData = GCHandle.ToIntPtr(_selfHandle);

#if NET8_0_OR_GREATER
        var status = NativeMethods.mediaway_device_hotplug_register_callback(_handle, &NativeCallback, userData);
#else
        var status = NativeMethods.mediaway_device_hotplug_register_callback(_handle, CallbackDelegate, userData);
#endif
        if (status != MediawayDeviceStatus.Ok)
        {
            _selfHandle.Free();
            MediawayDeviceException.ThrowIfError(status);
        }

        _callbackRegistered = true;
    }

    /// <summary>Caller must already hold <see cref="_gate"/>.</summary>
    private void UnregisterCallback()
    {
        // Blocks for up to the bridging thread's poll interval plus any in-flight callback
        // invocation's duration — a real, non-instantaneous cost the native header
        // documents rather than hides (same as Dispose/close below).
        MediawayDeviceException.ThrowIfError(NativeMethods.mediaway_device_hotplug_unregister_callback(_handle));
        _callbackRegistered = false;
        if (_selfHandle.IsAllocated)
        {
            _selfHandle.Free();
        }
    }

#if NET8_0_OR_GREATER
    [UnmanagedCallersOnly(CallConvs = new[] { typeof(System.Runtime.CompilerServices.CallConvCdecl) })]
#endif
    private static unsafe void NativeCallback(nint userData, NativeDeviceEvent* @event)
    {
        // MUST NOT throw past this point — an exception escaping an UnmanagedCallersOnly
        // method (net8.0) or a classic-marshalled delegate thunk (netstandard2.0) invoked
        // from native code terminates the process instead of propagating anywhere useful.
        // This is the C# analog of the native header's own "must not unwind across the FFI
        // boundary" callback contract — see this type's remarks.
        try
        {
            if (GCHandle.FromIntPtr(userData).Target is not DeviceHotplug self)
            {
                return;
            }

            // `event` is borrowed, valid only for the duration of this call — copy every
            // field (including a deep copy of the device-id string) before returning; do
            // not retain the pointer or call mediaway_device_hotplug_event_free on it (the
            // native bridging thread frees it itself immediately after this call returns).
            var args = ToManaged(*@event);
            self._deviceChanged?.Invoke(self, args);
        }
        catch
        {
            // Swallowed, deliberately — see this method's leading comment. There is no
            // error-reporting channel back through the native callback either way.
        }
    }

    /// <summary>
    /// Null-terminated UTF-8 C string -&gt; managed string, or <see langword="null"/> for a
    /// null pointer. <c>Marshal.PtrToStringUTF8</c> only exists from netstandard2.1/net5.0+ —
    /// this hand-rolled netstandard2.0 fallback walks the buffer itself instead of adding a
    /// dependency for one call site (see docs/adr/0018-csharp-netstandard20-unity.md).
    /// </summary>
    private static unsafe string? PtrToStringUtf8(nint ptr)
    {
#if NET8_0_OR_GREATER
        return Marshal.PtrToStringUTF8(ptr);
#else
        if (ptr == 0)
        {
            return null;
        }

        var p = (byte*)ptr;
        int len = 0;
        while (p[len] != 0)
        {
            len++;
        }

        return System.Text.Encoding.UTF8.GetString(p, len);
#endif
    }

    private static DeviceChangedEventArgs ToManaged(NativeDeviceEvent native) => new()
    {
        ChangeType = native.EventKind,
        Kind = native.DeviceKind,
        DeviceId = PtrToStringUtf8(native.DeviceId),
    };

    /// <summary>
    /// Closes this watcher — implicitly unregisters the native callback first if one is
    /// active (same join cost as <see cref="DeviceChanged"/>'s last <c>-=</c>), then closes
    /// the handle. Blocks for up to the ~50ms poll interval — a real, non-instantaneous
    /// cost the native header documents rather than hides. Always safe to call, including
    /// on a poisoned handle.
    /// </summary>
    public void Dispose()
    {
        lock (_gate)
        {
            if (_disposed)
            {
                return;
            }

            _disposed = true;
        }

        // Outside the lock: mediaway_device_hotplug_close itself performs the implicit
        // unregister-and-join (adr/0002-callback-event-delivery.md §4) — blocks until any
        // in-flight callback invocation has returned and the bridging thread is joined.
        // Running this under _gate would risk a handler that (in violation of the
        // documented contract) tries to touch this instance during teardown deadlocking
        // against this same lock.
        _handle.Dispose();

        // Only safe to free *after* the native close above has returned — that call
        // guarantees no thread is still inside NativeCallback dereferencing this handle.
        // Freeing it any earlier would race a still-in-flight callback's
        // GCHandle.FromIntPtr(userData) against this Free() call.
        if (_selfHandle.IsAllocated)
        {
            _selfHandle.Free();
        }
    }
}
