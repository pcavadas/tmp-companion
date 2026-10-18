# TMP Companion — overview

A macOS-first Tauri 2 desktop app (Rust backend + React/TypeScript frontend) that controls a Fender Tone Master Pro over USB. It renders its own UI and talks to the device with an exclusive HID session (the seize on macOS, share mode 0 on Windows, `flock` on Linux). macOS is the primary shipping platform; Linux also ships, as an **alpha** `.deb`/`.rpm` (see `notes/linux-packaging.md`), and Windows as an **unsigned** NSIS `setup.exe` (see `CONTRIBUTING.md`). Both are supported development platforms, including re-amp — see `CONTRIBUTING.md`.

> **This file is the architecture map.** `CLAUDE.md` deliberately does not carry a module tree — the per-module docs are the authority (see [Where things live](#where-things-live)). The UI is **click-only**: no keyboard shortcuts, no command palette (Enter/Escape inside a focused text input is exempt — see the carve-out in `CLAUDE.md`).

## Tabs

- **Level** — measures a preset's loudness by re-amping a synthetic stimulus through its DSP chain, then sets the `presetLevel` (and, per scene, the active amp's `outputLevel`) to hit a target LUFS. See `leveling.md`.
- **Doctor** — tone diagnosis: re-amps each selected sound (Base with all block-acting footswitches forced off, each scene, each footswitch in isolation), runs spectral checks (boomy/harsh/lost…), and says what to change on the unit — it never writes (the owner's decision). See `doctor.md` (feature doc; `benchmarks.md` covers the held-out accuracy benchmarks for the Doctor and leveling).
- **Copy** — copies signal-chain blocks from one reference preset into other presets, with per-target placement (replace / insert before|after / remove). See `block-copy.md`.
- **Songs** — device-backed songs and setlists CRUD (the unit is the source of truth). See `songs-setlists.md`.
- **Catalog** — a device-independent reference catalog of amps/cabs/effects with per-block art and CPU cost.
- **Settings** — instrument profiles, loudness targets, playback-level compensation, and dry-instrument calibration.

## Data paths

- **LIVE** — USB commands to the connected device (load preset/scene, set levels, rename/move/clear, song/setlist writes, live block edits).
- **OFFLINE** — the `.preset` file format for importing/re-importing a full preset; the OFFLINE `.preset` file is the canonical full-preset source (USB reads return a partial). See `write-safety.md`.
- **MEASURE** — re-amp capture + LUFS/spectrum analysis used by leveling and the analysis commands.

## Platform constraints

- **Ships:** macOS 12+, universal (Apple Silicon & Intel) — signed and notarized. Also an **alpha** Linux `.deb`/`.rpm` (see `notes/linux-packaging.md`) and an **unsigned** Windows NSIS `setup.exe`, via an `imp` seam on each of the two platform-boundary modules — hidraw / Win32 HID (`hid.rs`) and ALSA `hw:` / WASAPI shared mode (`audio.rs`); see their own headers for the measured detail (device resolution, format, the WirePlumber rule Linux dev machines need, the Windows default-format and endpoint-volume rules).
- Exclusive HID seize on macOS blocks Pro Control — while Pro Control holds the unit, the device tabs show "Another app is using your Tone Master Pro" (the connect error's `busy` kind). On Windows the share-mode-0 open does the same against Pro Control. On Linux `flock` locks out only another TMP Companion process; Fender ships no Pro Control for Linux.
- The device is single-connection: every device command is serialized through a process-global lock.
- Behaviour is firmware-version dependent; validated on fw 1.8.58, the support floor.

## Where things live

- Backend: `src-tauri/src/` — `hid.rs` (seize), `session/` (handshake + commands; `mod.rs` maps its modules, and only the transport set `link`/`timing`/`handshake`/`backup_stream`/`diag` touches the wire clock), `proto.rs` (wire codec), `monitor.rs` (live session + startup snapshot), `leveller/` (leveling, one file per lane — base, scene, footswitch, trade, rebalance, block — over shared `load`/`measure`/`knob`/`secant`/`persist`/`reamp_off`/`doctor_capture` seams; `mod.rs` maps them) / `lufs.rs` / `audio.rs` (measurement), `audiograph.rs` (node ops), `scene_jobs.rs` (scene-leveling job planning) / `stimulus.rs` (stimulus + WAV I/O, dry-DI capture) — shared logic the commands, the leveller and the probe all call, `commands/*.rs` (the `#[tauri::command]`s, one module per feature area), `app_commands.rs` (the ONE list of registered commands, shared by `bootstrap.rs`, the e2e server and `build.rs`), `bootstrap.rs` (builder/registration), `probe_api/*.rs` (the probe's entry points and probe-only helpers; nothing outside the probe imports from it), `song_io.rs` / `setlist_io.rs` (song and setlist reads), `discovery.rs` (active-graph and level-block discovery), `e2e_seed.rs` (the online-e2e scenario seed), `device_gate.rs` (device-op serialization), `slot.rs` (the 0-based list index and 1-based device slot as two types), `blockcaps.rs` (the sole pre-flight enforcement of the block-count caps and CPU budget), `replace_inplace.rs` (in-place preset re-import that keeps a slot's Song link), `lib.rs` (module wiring + a few e2e-feature-gated commands; no glob re-exports), `fixture_gates.rs` (the static fixture and gate-map tests).
- Frontend: `src/` — `views/` (one folder per tab), `device/` (the app-wide device stores: the one-per-connection library scan + live device state), `lib/invoke.ts` (typed command wrappers) + `lib/commands.ts` (the command registry: each command's args and return) + `lib/types.ts` (wire types), `ui/` (primitives + block art), `models/` (catalog data).
- **Command surface:** `app_commands.rs` registers exactly the commands `src/lib/invoke.ts` calls, and `src-tauri/capabilities/default.json` grants exactly those (`allow-<command>`); `src-tauri/src/command_surface_tests.rs` fails when the three drift. A command no screen calls is deleted, not kept registered and denied: adding one means its `invoke.ts` wrapper, its grant and its entry, together. The shipped build also sets a CSP (`tauri.conf.json`), so the fonts are bundled (`src/theme/fonts.ts`) and the only remote origin is the bug-report endpoint.
- The `probe` and `gen_samples` binaries (`src-tauri/src/bin/`) are the headless hardware-revalidation and stimulus-generation tools.

### Module docs are the authority

**Read the module's own header rather than any prose summary.** Nearly every backend file carries a `//!` header and nearly every frontend file a `//` header — between them that is the per-module documentation, kept next to the code it describes so it cannot drift the way a central tree does.

### Modules beyond the 6-tab UI

A set of bulk/offline feature modules is reachable only via the `probe` bin and the tests, and is **not** wired into any tab (they have no app command; see "Command surface" above): the bulk-run engine, bulk param edit, IR relink, block library, footswitch batch edit, per-scene amp pick, library search and offline preset-meta edits. Their `//!` docs are authoritative; there is no summary of them elsewhere.

### Reserved, uncalled

There is no module-wide `#[allow(dead_code)]`: a production item with no caller carries an item-level `#[expect(dead_code, reason = "…")]` (usually `cfg_attr(not(test), …)` when only a test exercises it), so the marker warns once the item is used again. The reserved ones are the golden-tested wire builders with no live caller (`export_preset_request`, `request_next_empty_preset_slot` / `next_empty_preset_slot_response`). Code only a test runs (`Session::harvest_slot_read`, which only the wire golden runs) is `#[cfg(test)]` instead.
