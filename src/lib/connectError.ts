// Classifies a `connect_device` failure into "show the red banner" vs "show the
// friendly please-connect gate". Returns the message to surface in the red
// AlertBanner, or `null` to suppress it (the device just isn't ready — the gate
// + the 3 s retry loop handle that).
//
// Two suppressed (gate, not error) cases:
//   • "no TMP found …"  — nothing plugged in yet.
//   • "IOHIDDeviceSetReport failed: …" — the unit is present and seized, but its
//     USB stack isn't accepting reports yet. This is the COLD-BOOT window: the
//     HID interface enumerates (so the hotplug watcher fires + the open/seize
//     succeeds) seconds before tm-stomp-server is ready, so the first handshake
//     send times out (kIOReturnTimeout, 0xe00002d6). It also covers the unit
//     being pulled mid-operation. Either way the fix is "wait / power it on" —
//     exactly what the gate says — not a red error, and the retry reconnects
//     once the unit is up.
//
// Everything else surfaces (red banner) — most importantly the actionable
// "close Fender Pro Control" hint, which comes from IOHIDDeviceOpen (a distinct
// message) when Pro Control holds the exclusive seize.
export function actionableError(raw: string): string | null {
  if (/no TMP found/i.test(raw)) return null;
  if (/IOHIDDeviceSetReport failed/i.test(raw)) return null;
  return raw;
}
