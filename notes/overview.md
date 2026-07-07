# TMP Companion — overview

A macOS-only Tauri 2 desktop app (Rust backend + React/TypeScript frontend) that controls a Fender Tone Master Pro over USB. It renders its own UI and talks to the device with an exclusive-seize HID session.

## Tabs

- **Level** — measures a preset's loudness by re-amping a synthetic stimulus through its DSP chain, then sets the `presetLevel` (and, per scene, the active amp's `outputLevel`) to hit a target LUFS. See `leveling.md`.
- **Doctor** — tone diagnosis: re-amps each selected sound (Base with all block-acting footswitches forced off, each scene, each footswitch in isolation), runs spectral checks (boomy/harsh/lost…), and offers one-click fixes. See `doctor-calibration.md`.
- **Copy** — copies signal-chain blocks from one reference preset into other presets, with per-target placement (replace / insert before|after / remove). See `block-copy.md`.
- **Songs** — device-backed songs and setlists CRUD (the unit is the source of truth). See `songs-setlists.md`.
- **Catalog** — a device-independent reference catalog of amps/cabs/effects with per-block art and CPU cost.
- **Settings** — instrument profiles, loudness targets, playback-level compensation, and dry-instrument calibration.

## Data paths

- **LIVE** — USB commands to the connected device (load preset/scene, set levels, rename/move/clear, song/setlist writes, live block edits).
- **OFFLINE** — the `.preset` file format for importing/re-importing a full preset; the OFFLINE `.preset` file is the canonical full-preset source (USB reads return a partial). See `write-safety.md`.
- **MEASURE** — re-amp capture + LUFS/spectrum analysis used by leveling and the analysis commands.

## Platform constraints

- macOS 12+ on Apple Silicon. The IOKit HID seize and cpal CoreAudio paths are `cfg(target_os = "macos")`.
- Exclusive HID seize blocks Pro Control — the app surfaces a "close Pro Control" error if it is running.
- The device is single-connection: every device command is serialized through a process-global lock.
- Behaviour is firmware-version dependent (validated on 1.7.75 and 1.8.45).

## Where things live

- Backend: `src-tauri/src/` — `hid.rs` (seize), `session.rs` (handshake + commands), `proto.rs` (wire codec), `monitor.rs` (live session + startup snapshot), `leveller.rs` / `lufs.rs` / `audio.rs` (measurement), `audiograph.rs` (node ops), `commands/*.rs` (the `#[tauri::command]`s, one module per feature area), `bootstrap.rs` (builder/registration), `probe_api/*.rs` (probe entry points), `device_gate.rs` (device-op serialization), `lib.rs` (module wiring + a few e2e-feature-gated commands).
- Frontend: `src/` — `views/` (one folder per tab), `lib/invoke.ts` (typed command wrappers) + `lib/types.ts` (wire types), `ui/` (primitives + block art), `models/` (catalog data).
- The `probe` and `gen_samples` binaries (`src-tauri/src/bin/`) are the headless hardware-revalidation and stimulus-generation tools.
