---
paths:
  - "src-tauri/src/bin/probe.rs"
  - "src-tauri/src/probe_api/**"
  - "src-tauri/src/sim_device.rs"
---

# Probe and SimDevice rules

The `probe` arms drive the real unit and `sim_device.rs` stands in for it offline, so both follow the device facts in [`notes/device/`](../../notes/device/):

- [`session.md`](../../notes/device/session.md): the seize, the 750 ms session lapse, request batching, the handshake and reply queues.
- [`presets.md`](../../notes/device/presets.md): slot addressing, reads, saves, imports, rejected bodies, structural edits and the insert abort.
- [`scenes.md`](../../notes/device/scenes.md): scene context, scene-write landing and recall behaviour.
- [`reamp-audio.md`](../../notes/device/reamp-audio.md): the re-amp toggle, engage and capture accuracy, silent captures and stuck re-amp.

Run a probe binary from `<worktree>/src-tauri` with the unit plugged in and Pro Control closed; the danger rules are in [`danger.md`](danger.md).
