---
paths:
  - "src-tauri/src/session/**"
  - "src-tauri/src/hid.rs"
  - "src-tauri/src/hid/**"
  - "src-tauri/src/monitor.rs"
  - "src-tauri/src/device_gate.rs"
---

# Device session rules

Read [`notes/device/session.md`](../../notes/device/session.md) before changing how a session opens, waits, batches or reconnects.
Its sections are the authority for the transport, the exclusive seize, the 750 ms session lapse, `batchStatus` request batching, the handshake, the two reply queues, the boot window and the monitor.
Wire encoding (field tables, framing) is the `tmp-companion-protocol` skill; the danger rules are in [`danger.md`](danger.md).
