// list-and-watch-devices.ts — enumerate media devices and react to hotplug.
//
// REAL — device enumeration is a native browser capability in this host (Tier
// C: WASM + Web APIs; the C ABI is never involved). Mediaway's WASM module
// does not wrap it; the browser owns it via
// navigator.mediaDevices.enumerateDevices() + the devicechange event. This is
// the browser-native analog of mediaway-device's DeviceId / Select / hotplug
// vocabulary (crates/mediaway-device/adr/0005-device-selection.md): the
// persistent deviceId plays the role of DeviceId, and devicechange is the
// hotplug notification — no Rust/wasm involved, pure native Web API, nothing
// to wrap.
//
// Run: open in a Chromium-based browser; plug/unplug a device to see change
// events.

async function main(): Promise<void> {
  // Labels are empty until a capture permission is granted; deviceId is still
  // populated and stable across calls (the DeviceId analog).
  const known = new Map<string, string>(); // deviceId -> kind
  for (const d of await navigator.mediaDevices.enumerateDevices()) {
    known.set(d.deviceId, d.kind);
    console.log(`device: ${d.kind} "${d.label || "(unlabeled)"}" (${d.deviceId})`);
  }
  console.log(`${known.size} device(s); watching for plug/unplug…`);

  navigator.mediaDevices.addEventListener("devicechange", async () => {
    const devices = await navigator.mediaDevices.enumerateDevices();
    const now = new Set(devices.map((d) => d.deviceId));
    for (const [deviceId, kind] of known) {
      if (!now.has(deviceId)) {
        console.log(`device removed: ${kind} ${deviceId}`);
        known.delete(deviceId);
      }
    }
    for (const d of devices) {
      if (!known.has(d.deviceId)) {
        console.log(`device added: ${d.kind} "${d.label || "(unlabeled)"}" (${d.deviceId})`);
        known.set(d.deviceId, d.kind);
      }
    }
  });
}

main().catch((err) => {
  console.error(err);
});
