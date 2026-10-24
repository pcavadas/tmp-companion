---
paths:
  - "e2e/**"
  - "scripts/e2e.sh"
  - "scripts/hw-e2e.sh"
  - "src-tauri/src/e2e_seed.rs"
  - "src-tauri/src/e2e_server.rs"
  - "src-tauri/src/e2e_server_tests.rs"
  - "src-tauri/src/fixture_gates.rs"
---

# e2e harness rules

Applies while editing the Playwright harness or its specs.
The stale-server false-green trap and the stamp rules are in [`AGENTS.md`](../../AGENTS.md), because they fire when you run the suite.

## Why the harness is shaped this way

Tauri's `tauri-driver`/WebdriverIO cannot drive this app — macOS WKWebView has no WebDriver.
So the harness drives the **real React UI in headless Chromium** → an HTTP bridge → a windowless Rust backend (`tauri::test::mock_builder` MockRuntime, `bin/e2e_server.rs`) → `SimDevice` offline, the real device online (`TMP_E2E_ONLINE=1`), or a remote device (`TMP_E2E_REMOTE=<addr>`, `hid::remote`).
One spec set runs under three configs: offline, online and remote.
Vitest owns component-level coverage with the invoke/event bridge mocked; use the `probe` bin for real-HID paths.

**The remote tier.**
Its re-amp captures go to the remote device's own audio over `remote_audio` when `TMP_E2E_REMOTE_AUDIO=<addr>` is set beside `TMP_E2E_REMOTE`, and are refused without it.
The remote device checks each capture's engagement (a failed engage is an error; counters at `/remote/audio`).
`/health` reports `online`, `remote` and `remoteAudio`, and every non-remote lane refuses a remote server.
Its lane is `scripts/e2e-remote.sh`, which never stamps: remote captures are not calibrated against the unit, so they are not device evidence.

**Because it is one spec set, the configs' `timeout:` caps must stay EQUAL** (300 s).
A spec's per-assertion `timeout:` is written for the slower mode (the leveling waits declare 240 s), and a config whose cap is lower silently truncates them — the test dies mid-flight with a bare "Test timeout of Nms exceeded" pointing at whichever assertion was unlucky, never at the assertion that actually failed.

## How to run

`scripts/e2e.sh` is the turn-key runner (`bun run e2e` runs its offline mode); its header lists every mode.

- **`bash scripts/e2e.sh`**: offline, all specs against the SimDevice (~1.5 min, no hardware).
- **`bash scripts/e2e.sh online`**: the real unit, one spec at a time (doctor.online, level.online, songs, copy; ~25 min).
  Plug the unit in and close Pro Control (it holds the exclusive HID seize).
  The script pre-flights the handshake and recovers the unit on every handled exit (`trap cleanup EXIT INT TERM`): re-amp off, a stray sweep and a recall of preset 001, plus a guarded clear of the scenario slots (`SCRATCH_SLOTS`) only with `TMP_E2E_CLEAR_SCENARIO=1`.
  A SIGKILL or host crash skips the trap: recover with `cargo run --bin probe -- --reamp-off`.
- **`bash scripts/e2e.sh online auto`**: exactly the specs this diff requires (`scripts/gates.sh --required-online`); a diff that requires none exits 0 without touching the unit.
- **`bash scripts/e2e.sh online release`**: the same over everything merged since the last release tag; `scripts/pre-release.sh` wraps it for the release-time run.
- Add a spec name (`copy`, `songs`, `doctor.online`, `level.online`) to run one spec, e.g. `bash scripts/e2e.sh online level.online`.

Any passing online run merges the specs it ran into the tree's online stamp (one `#run` line per run, with `os=`, `fw=` and `unit=`), so a failed spec re-runs alone.
Soak never stamps, a level.online run stamps only when the ffmpeg validation passed, and no run stamps unless the unit presents its stock USB interfaces.
Per-spec wall times land in `$TMPDIR/tmp-companion-e2e/online-timings.tsv`, with the spec's hardware waits (settle and capture seconds, from the server's `GET /timing`, `rig::WAITS`) beside them.
Offline, `scripts/e2e-budgets.mjs` prints and persists (`e2e/test-results/e2e-phases.json`) the build / run / test-time split.

## UI copy is e2e-load-bearing

The specs **regex-match user-facing strings** (`doctor.spec.ts` matches `/sounds? needs? a look|^Nothing to change in|check couldn.t finish/`).
Before rewording any view label or heading — especially on a design handoff — **grep `e2e/specs/` for the old phrase first**.

## Events and Channels reach the page (the UI bridge)

Per-scene/per-footswitch leveling **results stream via a Tauri `Channel`**, and backend `tmp://` events (live sync, attach/detach, backup progress, the run meter) are `emit`s.
Under MockRuntime neither reaches a webview, so `e2e_ui_bridge.rs` captures both in-process (the Builder's `channel_interceptor` and one `listen_any` per event name in `UI_EVENTS`) into a bounded ring served at `GET /ui/events?after=<seq>` on the side lane.
`e2e/bridge-client.js` polls it while the page holds a listener or an in-flight Channel, and an invoke that carried a Channel polls once more before resolving, so every streamed row lands before the return value.
**A rendered row outcome is therefore UI-observable in both tiers.**
A new `tmp://` event the backend emits must join `UI_EVENTS`, or the page never hears it.

Two limits: a Channel's end-of-stream notification is not captured, and a spec's own raw `invoke()` (`page.request.post`, `fixtures/scenario.ts`) carries no Channel, so its rows go nowhere.
For a raw invoke the observation path stays the command's RETURN value plus the SimDevice event log (`/sim/events`, offline-only) for the wire writes, never a re-derivation.
The Rust gates in `e2e_server_tests.rs` prove the physics on the return value; the Playwright specs prove the bridge, the command layer and the rendered rows.

## Seeding and list reads

- Seed-path list reads are **TOLERANT plus a completeness floor, never `list_my_presets_strict`**.
  Strict decodes only terminal-frame streams and fails or garbles on back-to-back lean sessions [HW].
- Online seeding runs a **FRESH `probe --seed-scenario` process BEFORE the server starts**, so the server's own handshake snapshots the seeded presets.
  The seed self-repairs by sweeping stray imports — an aborted seed strands copies at the first empty slot anywhere in the bank.
- The eleven scenario presets live in the scratch zone at list indices 400–410 and **stay resident between runs by default** — the pristine-checking seed re-imports any drifted or stale-rev slot.
  Teardown unconditionally disables re-amp, sweeps strays and recalls preset 001 (an empty sweep leaves the server's `SCENARIO_VERIFIED` set, so the next spec's `ensureScenario` skips the re-verify unless a structural save cleared it), but clears the scenario slots **only** with `TMP_E2E_CLEAR_SCENARIO=1`, for an on-demand net-zero run.
  **Their shapes are deliberate and the per-use-case map is [`e2e/fixtures/COVERAGE.md`](../../e2e/fixtures/COVERAGE.md)** — read it before changing a fixture, and update it in the same commit.
  In brief, by list index (the device slot is list index + 1):
  - `E2E Rig` (400) — the scene-overlay + footswitch + Doctor-damage fixture.
  - `E2E Pedalboard` (401) — the scene-free copy/import + EXP/link-group fixture.
  - `E2E Edge` (402) — the split-output 8-scene fixture; also carries the Doctor's baked 2.6 kHz EQ-ring oracle.
  - `E2E Parallel` (403) — the both-lane-amps joint-k fixture.
  - `E2E Hiwatt 3S` (404) — a **verbatim device export** backing the wipe/bake/measurement-context gates (its exact byte length is pinned — do not edit it).
  - `E2E Preset24` (405) — the preset-24 saturated-pedal footswitch fixture (`e2e_server_tests.rs::the_footswitch_rows_still_converge_after_a_base_boost`, and `level.online.spec.ts`'s Plumes-shape first-run journey).
  - `E2E Combined Level` (406) — the combined leveling fixture (FS-alone / scene-alone "BASE SCENE" / scene-that-enables-an-FS, parallel both-amps-active, a post-cab compressor).
  - `E2E Doctor Oracle` (407) — 13 param-shaped footswitches, one per Doctor spectral check, its defect blocks enabled-neutral in base.
  - `E2E Preset24 Min` (408) / `E2E Hiwatt Min` (409) — the smallest presets still reproducing each incident's own bug class.
  - `E2E Friedman 3S` (410) — the leveling-regression fixture: `ACD_TubeScreamer` (base-ON) → `ACD_MarshallPlexi` (the catalog-verified stand-in for a Friedman HBE-class amp) → `ACD_CabSimTMS`, 3 FULL-overlay scenes (Rhythm/Lead/Base Scene), no `leveledParams` (a plain flat-C/`ol_term` fixture).
- **THE CAB RULE (standing user directive):** every guitar amp in every fixture is an amp+cab-merged model (a combo or half-stack, a cab/IR-suffixed id) with NO cab after it, or a bare head with a cabinet block DOWNSTREAM IN ITS OWN LANE; no preset holds more than two cabs/IRs/mics.
  Enforced by `fixture_gates::every_guitar_amp_in_every_fixture_reaches_a_cab` and `every_fixture_holds_at_most_two_cabinet_slots` ([`notes/e2e-fixtures.md`](../../notes/e2e-fixtures.md#the-cab-rule)).
  Offline the `scenario-loudness.json` C table _is_ the model — each entry's `_comment` records what its C folds in; the real unit's own loudness is measured, never read from the table.
- **Fixtures are generated, not hand-edited in place, and every regen bumps `FIXTURE_SOURCE_STAMP`'s `#rN` suffix** (`e2e_seed.rs`) — a resident copy of an older rev must fail the pristine check and self-migrate.
  A regen also means rerunning `cargo test build_scenario_fixture -- --ignored` to rebuild `backup-fixture.bin`.

## Online preconditions — the unit's own global settings count

Beyond "plugged in, Pro Control closed": the device's global **Scene Change Behavior must be MAINTAIN CHANGES** for any spec set containing `level.online`.
That spec drives the DEFERRED-WRITE leveling lanes (`level_scenes_apply_batched`, `level_footswitches_apply`), which `level_scenes::scene_discard_guard` refuses outright under DISCARD — so the run dies ~13 min in, after `doctor.online` has already spent ~8 minutes, and level.online's Plumes journey runs after it on the same misconfigured unit.
It is a device misconfiguration, not a platform bug: any host fails identically.

`scripts/e2e.sh`'s `scene_change_preflight` catches it in seconds instead.
Two things about it are load-bearing and must not be "simplified":

- **It REFRESHES the snapshot before reading it** (`read_library_via_backup`, read-only, ~20 s).
  The snapshot is only ever written by a backup scan, so a pre-flight that aborted on a stale DISCARD file would never rewrite it — the user could fix the touchscreen, replug, and still be refused forever.
- **It reads a SERVER LOG line, not the snapshot file.**
  That path is `app_config_dir()`-derived and varies by OS _and_ bundle identifier (the e2e_server and the app can write different directories), so locating it from shell is fragile.
  The line comes from `commands::presets::persist_device_settings`; its three verdict words (`DISCARD`/`MAINTAIN`/`unknown`) are a contract between the two.

## `scripts/hw-e2e.sh` — the attended on-device layer

Runs the full Level + Copy happy paths against the real unit **non-destructively** (dry `--levelpreset` with no save, `--replace-held` with no commit, `--device-backup` read, `--reamp-off`).
Override its `LEVEL_SLOT` / `COPY_*` env vars per unit.
It is **attended, not a CI gate**, and acquires the machine-global device lock like the online `e2e.sh` path.

## External validation — ffmpeg is the only trusted meter here

`scripts/level-validate.sh` judges a leveled-and-saved sound's captured WAV with ffmpeg's `ebur128` filter against the TARGET the leveling run recorded — **never** against that run's own `predicted_lufs`/`verify_lufs`.
The point is a meter this repo did not write: a `lufs.rs` regression that fools the Rust unit tests would still fool a self-consistency check, but not an independent BS.1770 implementation.

**Gate semantics** (ffmpeg absent is advisory, present with rows is a real gate) are canonical in `scripts/e2e.sh`'s header.

**What runs when:**

- **`scripts/e2e.sh online …`** exports `TMP_E2E_VALIDATE_LOG` and `TMP_E2E_VALIDATE_WAV_DIR` into the e2e_server's environment (that process runs the strict re-measures and dumps each WAV), then runs `level-validate.sh --expectations <log>` over them after the spec loop.
  The verdict is `lane_external_validation` (`scripts/lib/e2e-lane.sh`, proven by `scripts/e2e-lane.spec.sh`): a run certifies only when no row was capped and every identity in `level.online.spec.ts`'s `STRICT_VALIDATE_LABELS` was measured and passed, read from the judge's `--summary`; a passing prefix never certifies.
- **`scripts/validate-hbe.sh <preset-file>`** — the attended, standalone Friedman-HBE run: import → level (base + optional scenes/footswitches) → re-measure with `--target`/`--dump-wav` → **completeness check** → `level-validate.sh --expectations` → clear, with a trap-guaranteed re-amp OFF (gap, one retry after a longer quiet, then a loud `probe --reamp-off` banner) on every exit path.
  The **completeness check is not optional bookkeeping**: a re-measure that dies before its capture emits NO row, and the judge can only grade rows it is handed — a shorter log is invisible to it.
  The script therefore records the label of every row it ASKS for and greps the log for each by identity (not by count, which a duplicate would mask) before calling the judge; a missing row is exit 1 whatever the judge said, including when the judge skipped for want of ffmpeg.
- **`scripts/level-validate.sh`** is the shared judge both callers use.
  `--expectations <jsonl>` (batch, the automated path), `--wav <path>` (one file, with `--probe-log` for the FLOOR/SILENT proof), or `--live <seconds>` (bare avfoundation capture — ATTENDED ONLY: it has no engage-proof, so the caller must verify engagement by other means).

**The emission seam:** `crate::validate_log`, appended to from the **measurement** seams — `leveller::measure_sound_asis_strict` (driven online by `e2e_measure_sound`) and `probe_api::level::probe_measure_current_lufs` (`probe --measure-scene --target … --dump-wav`).
Not from the leveling run: the solve captures at its REFERENCE level, so its PCM is not the saved preset's output.
Each row is written by the same capture that produced the run's own number, so it carries the WAV path, the engage verdict, and the sound's own **identity** (`slot` + `scene_slot`/`switch`) — never a position.
That is why `LevelResult` carries `scene_slot`: `level_scenes_apply_batched` filters failed scenes out of the array it returns, so index _i_ is not scene _i_ once anything fails.

**Both probe emission seams resolve their stimulus through `probe_stimulus_path(topology_id)`** — the bundled topology WAV, consulting neither the profile's captured DI nor `TMP_LEVELLER_STIMULUS`, unlike the leveling arms that honour both.
Unless both sides are pinned to one WAV, a run and its "independent" re-measure grade different waveforms, and the difference is large enough to move a verdict.

Footswitch rows are externally validated through `probe --measure-footswitch <slot> <switch> <topology> [--lev g:n:p] [--target L] [--dump-wav D]`.
Doctor writes nothing to the unit, so there is no Doctor write path to validate.

**Env vars:**

- `TMP_E2E_VALIDATE_LOG` — jsonl expectations path; `e2e.sh` sets it when ffmpeg is present.
  UNSET means every emission seam is a no-op.
- `TMP_E2E_VALIDATE_WAV_DIR` — WAV dump dir; defaults to a `level-validate-wavs` sibling of the log.
- `TMP_E2E_VALIDATE_MAX_ROWS` — row cap for the e2e pass, default 40.
- `TMP_E2E_LEVEL_TOL_LU` — validation tolerance in LU, default **1.0**.
  It must exceed the solver's own acceptance band of 0.3 LU plus recapture noise, or correct runs fail.
- `TMP_E2E_AVF_DEVICE` — avfoundation device id for `--live`, default `:0`.

**`level-validate.sh` exit codes, which both callers branch explicitly:** `0` every row passed (at least one actually measured) · `1` at least one row failed · `2` usage error · `3` ffmpeg absent, nothing checked · `4` **vacuous** pass — zero measured rows (every row clamped), announced in a yellow `PASS (VACUOUS)` banner.
A `persist_mismatch: true` row is a FAIL (`1`), never a skip: a save that did not persist is the regression the judge exists to catch.
A `3` must be reported as SKIPPED, never as a target miss.
A `4` is not a failure, but it must never certify: the online lane treats it as "passed but NOT stamped" — a skip is a real verdict, but it is not verification.

## Run evidence and traces

Online and soak runs archive into run folders under the git common dir, and the seed probe and e2e server write wire traces there: the layout, the TCT v1 event table, the env vars and the privacy rules are in [`notes/e2e-recording.md`](../../notes/e2e-recording.md).
Never write lane output under the worktree (it re-keys the stamps), and never commit or upload raw evidence.
Only derived data is public (the CPU table `model-cpu.json`, the Empty template's observable fields); no descriptor files, factory preset bodies, default parameters, IRs, user content or real device backups are committed.
Device evidence that stamps, records or calibrates comes only from a unit presenting its stock USB interfaces.

## Fixtures

- The offline `backup-fixture.bin` and `scenario-presets.json` **must stay in sync** — regenerate both from one script.
- **Fixture drift-lock trap:** a drift-lock or round-trip test that compares fixtures **through a typed struct** silently covers only the fields that struct carries.
  `info.product_id` and `info.preset_id` are fields such a test misses.
  Assert fixture invariants against the **RAW JSON** (`fixture_gates.rs`, deliberately OUTSIDE `#[cfg(feature = "e2e")]` — a gate that only compiles in a build nobody makes is not a gate).
  It pins `product_id == "tmStomp"` (a `"pro"` preset is rejected on the unit as "created using a newer firmware revision") and a unique `preset_id` per fixture preset.
- The preset XOR key is committed as `const PRESET_XOR_KEY: [u8;3] = *b"JLD"` in `backup.rs`.
  **Never derive or learn the key at runtime.**

## Ports

`scripts/e2e.sh` derives a stable per-worktree port band and exports `TMP_E2E_PORT` / `TMP_E2E_VITE_PORT` / `TMP_E2E_WORKERS`; its header is canonical for the bands, the stride and why each lane owns its own Vite.
Each offline Playwright worker runs its own `e2e_server` on `TMP_E2E_PORT + parallelIndex` (`e2e/fixtures/port.ts`).
The stale-server sweep filters by process name (`e2e_server`) before killing, because `lsof -i tcp:N` cannot tell whose server it found; keep that filter.
The hash can still collide across worktrees; the one real device is serialized by a machine-global `mkdir` lock (`scripts/device-lock.sh`).

## Each offline server has its own store, fresh every run

The e2e_server builds on Tauri's mock context, whose empty identifier puts `profiles.json` (targets, instruments, the instrument chosen for each preset) straight in the user's config dir.
So the offline config gives each server its own `HOME` and `XDG_CONFIG_HOME` under `TMP_E2E_CONFIG_ROOT`, which `scripts/e2e.sh` creates fresh for every offline run and removes when it ends (a bare `bunx playwright test` makes its own in the system temp dir).
Without it the workers share one store with each other and with every earlier run, and Doctor's Instrument page silently stops showing after a machine's first run.
**A spec may count on an empty store at its server's start, never on one an earlier run left**: `doctor.spec.ts`'s first check requires the Instrument page (`runDoctorCheck(page, { expectInstrumentPage: true })`).
Windows finds the config dir through a system call no environment variable moves, so a Windows run still shares one store, and from its second run on that check fails there, loudly, rather than skip the page.
The online lane's single server, started by `cargo run`, keeps the shared store (a different `HOME` would hide cargo's own toolchain from it).
