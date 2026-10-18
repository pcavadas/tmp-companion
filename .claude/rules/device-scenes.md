---
paths:
  - "src-tauri/src/leveller/**"
  - "src-tauri/src/scene_jobs*.rs"
  - "src-tauri/src/footswitch.rs"
  - "src-tauri/src/commands/level_*.rs"
  - "src-tauri/src/session/reamp.rs"
---

# Scene and re-amp rules

Read the device sections before changing a scene recall, a scene or footswitch write, an engage or a capture.

- [`notes/device/scenes.md`](../../notes/device/scenes.md): scene context, Scene Change Behavior, scene-write landing, `presetLevel` and footswitch state on recall, scene index bounds, the batched scene save.
- [`notes/device/reamp-audio.md`](../../notes/device/reamp-audio.md): the re-amp toggle, engage and capture accuracy, silent captures, stuck re-amp, sample rate, output routing, the capture window.
- [`notes/leveling.md`](../../notes/leveling.md): the leveling model built on them.

The danger rules are in [`danger.md`](danger.md).
