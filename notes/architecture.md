# Architecture

TMP Companion is a Tauri 2 desktop app (Rust backend, React/TypeScript frontend) that controls a Fender Tone Master Pro over USB.
It draws its own UI and talks to the device over an exclusive HID session.
The UI is click-only: no keyboard shortcuts and no command palette; Enter/Escape inside a focused text input is the one exception (`AGENTS.md`).
Every other note is indexed in [`notes/README.md`](README.md).

## Tabs

Six tabs, routed by `src/App.tsx`, one folder per tab under `src/views/`:

- **Level** measures a preset's loudness by re-amping a synthetic stimulus through its DSP chain, then sets `presetLevel` (and, per scene, the active amp's `outputLevel`) to hit a target LUFS ([`leveling.md`](leveling.md)).
- **Doctor** re-amps each selected sound, runs spectral checks and says what to change on the unit; it never writes ([`doctor.md`](doctor.md)).
- **Copy** copies signal-chain blocks from one preset into others, with per-target replace / insert / remove ([`copy.md`](copy.md)).
- **Songs** edits the songs and setlists stored on the unit, which is the source of truth ([`songs-setlists.md`](songs-setlists.md)).
- **Catalog** is a device-independent reference of amps, cabs and effects with block art and CPU cost.
- **Settings** holds instrument profiles, loudness targets, playback-level compensation and dry-instrument calibration.

Level, Doctor, Copy and Songs show the shared `DeviceState` page until a unit connects; Catalog and Settings work without one.

## Data paths

- **Live**: USB commands to the connected unit (load preset or scene, set levels, rename / move / clear, song and setlist writes, live block edits).
- **Reads**: a USB read is whole when the session stays alive until its final frame; a session lapse cuts it ([device/presets.md#reading-presets](device/presets.md#reading-presets)).
  Startup state comes from one device backup scan (`backup_read.rs`).
- **Offline**: the `.preset` file codec (`backup::xor_jld`, `library::decode_preset_bytes`) for import and in-place re-import (`replace_inplace::replace_inplace_core`).
- **Measure**: re-amp capture plus LUFS and spectrum analysis (`audio.rs`, `lufs.rs`, `psd.rs`), used by leveling and the Doctor.

## Platforms

- **macOS 12+** is the shipping platform: one universal (arm64 + x86_64) build, signed and notarized ([`releasing.md`](releasing.md)).
- **Linux** ships as an alpha `.deb` / `.rpm` and **Windows** as an unsigned NSIS `setup.exe` ([`releasing.md`](releasing.md)); both are supported development platforms, re-amp included (`CONTRIBUTING.md`).
- Each platform boundary is an `imp` seam in one module: HID in `hid.rs` (IOKit seize, hidraw, Win32 HID) and audio in `audio.rs` (Core Audio, ALSA `hw:`, WASAPI shared mode); their headers carry the per-platform detail ([device/session.md#transports-by-platform](device/session.md#transports-by-platform), [device/reamp-audio.md#platform-audio](device/reamp-audio.md#platform-audio)).
- The HID open is exclusive: the macOS seize and the Windows share-mode-0 open lock out Pro Control, and the device tabs then say "Another app is using your Tone Master Pro"; on Linux `flock` locks out only a second TMP Companion ([device/session.md#exclusive-seize](device/session.md#exclusive-seize)).
- Every device command is serialized through one process-global lock (`device_gate.rs`).
- fw 1.8.58 is the support floor.

## Module map

Nearly every backend file carries a `//!` header and nearly every frontend file a `//` header; those headers are the authority, and this map only says where to look.

Backend, `src-tauri/src/`:

- `hid.rs` and `hid/`: device open, the exclusive seize, re-enumeration.
- `session/`: the handshake and every device command; `session/mod.rs` maps its modules, and only `link`, `timing`, `handshake`, `backup_stream` and `diag` touch the wire clock.
- `proto.rs`: the wire codec.
- `monitor.rs`: the live session and the startup snapshot.
- `device_gate.rs`: device-op serialization, the abort flag, and `with_released_seize_blocking`, which holds the re-amp run guard around every device command.
- `leveller/`: leveling, one file per lane (base, scene, footswitch, trade, rebalance, block) over shared `load` / `measure` / `knob` / `secant` / `persist` / `reamp_off` / `doctor_capture` seams; `leveller/mod.rs` maps them, and `reamp_off.rs` holds `ReampRunGuard`.
- `audio.rs`, `lufs.rs`, `psd.rs`: capture and measurement.
- `doctor.rs`: the pure Doctor rules; `commands/doctor.rs` drives the device.
- `audiograph.rs`: the node walks (`for_each_node`, `for_each_node_mut`, `Node::matches_instance`, `mint_node_id`) the scan and edit modules share.
- `scene_jobs.rs`: scene-leveling job planning and the scene-overlay classifier.
- `stimulus.rs`: stimulus resolution, WAV I/O and dry-DI capture.
- `blockcaps.rs`: the only host-side guard for the four block-count caps and the CPU budget, and the only enforcement of the count caps; every apply path calls `blockcaps::check_op`.
- `slot.rs`: the 0-based list index and the 1-based device slot as two types, and `SCRATCH_SLOTS`, the one declaration of the scratch zone.
- `replace_inplace.rs`: in-place re-import that keeps a slot's song link, and the load proof every import → load → save path uses.
- `backup.rs`, `backup_read.rs`, `library.rs`: the `.preset` codec and the backup scan.
- `song_io.rs`, `setlist_io.rs`: song and setlist reads.
- `discovery.rs`: active-graph and level-block discovery.
- `commands/*.rs`: the `#[tauri::command]`s, one module per feature area.
- `app_commands.rs`: the one list of registered commands, shared by `bootstrap.rs`, the e2e server and `build.rs`.
- `probe_api/*.rs`: the probe's entry points and probe-only helpers; nothing outside the probe imports from it.
- `e2e_seed.rs`, `e2e_server.rs`, `sim_device.rs`: the e2e harness and the offline device model.
- `fixture_gates.rs`: the static fixture and gate-map tests, including the `e2e/fixtures/COVERAGE.md` parser.
- `trace/`: the wire and capture recorder ([`e2e-recording.md`](e2e-recording.md)).
- `bin/probe.rs`, `bin/gen_samples.rs`: the headless hardware-validation tool and the stimulus generator.

Frontend, `src/`:

- `views/`: one folder per tab, plus `signalChain/` (the strip drawing) and `overlays/`.
- `device/`: the app-wide device stores (the one-per-connection library scan and live device state).
- `lib/invoke.ts` (typed command wrappers), `lib/commands.ts` (each command's args and return) and `lib/types.ts` (wire types).
- `ui/`: primitives and block art; `ui/primitives.tsx` stays one file because `Modal` renders `Button`, and `ui/Dialog.tsx` stays button-free so the import goes one way.
- `models/`: catalog data; `models/blockArt.ts` must not import `models/catalog.ts` (a module-init cycle, enforced in `eslint.config.js`).
- `theme/`: tokens and the bundled fonts.

## Command surface

`app_commands.rs` registers exactly the commands `src/lib/invoke.ts` calls, and `src-tauri/capabilities/default.json` grants exactly those (`allow-<command>`); `src-tauri/src/command_surface_tests.rs` fails when the three drift.
A command no screen calls is deleted, not kept registered and denied: adding one means its `invoke.ts` wrapper, its grant and its entry together.
The shipped build sets a CSP (`tauri.conf.json`), so the fonts are bundled (`src/theme/fonts.ts`) and the only remote origin is the bug-report endpoint.

## Probe-only modules

Some bulk and offline feature modules have no app command and are reachable only from the `probe` binary and the tests: the bulk-run engine, bulk parameter edit, IR relink, block library, footswitch batch edit, per-scene amp pick, library search and offline preset-meta edits.
Their `//!` headers are their only documentation.

## Uncalled items

There is no module-wide `#[allow(dead_code)]`.
A production item with no caller carries an item-level `#[expect(dead_code, reason = "…")]` (usually `cfg_attr(not(test), …)` when only a test exercises it), so the marker warns once the item is used again.
The reserved ones are golden-tested wire builders with no live caller (`export_preset_request`, `request_next_empty_preset_slot` / `next_empty_preset_slot_response`).
Code only a test runs (`Session::harvest_slot_read`, run only by the wire golden) is `#[cfg(test)]` instead.
