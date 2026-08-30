---
paths:
  - "e2e/**"
  - "scripts/e2e.sh"
  - "scripts/hw-e2e.sh"
---

# e2e harness rules

Applies while editing the Playwright harness or its specs. **The stale-server false-green trap is in `CLAUDE.md`** because it fires when you _run_ the suite, not when you read these files.

## Why the harness is shaped this way

Tauri's `tauri-driver`/WebdriverIO cannot drive this app — macOS WKWebView has no WebDriver. So the dual-mode harness drives the **real React UI in headless Chromium** → an HTTP bridge → a windowless Rust backend (`tauri::test::mock_builder` MockRuntime, `bin/e2e_server.rs`) → `SimDevice` offline, or the real device online with `TMP_E2E_ONLINE=1`. One spec set under two configs. Vitest owns component-level coverage with the invoke/event bridge mocked; use the `probe` bin for real-HID paths.

**Because it is one spec set, the two configs' `timeout:` caps must stay EQUAL.** A spec's per-assertion `timeout:` is written for the slower mode (eleven waits declare 240 s), and a config whose cap is lower silently truncates them — the test dies mid-flight with a bare "Test timeout of Nms exceeded" pointing at whichever assertion was unlucky, never at the assertion that actually failed. An offline cap of 120 s did this to `level-defaults.spec.ts`'s two-run fallback test, which reports as a red `e2e` on unrelated PRs.

## UI copy is e2e-load-bearing

The specs **regex-match user-facing strings** (`doctor.spec.ts` matches `/presets? need a look|All clear/`). Before rewording any view label or heading — especially on a design handoff — **grep `e2e/specs/` for the old phrase first**.

## The Channel-streaming seam (deliberate, not a bug)

Per-scene/per-footswitch leveling **results stream via a Tauri `Channel`**, and the offline
HTTP bridge (`bin/e2e_server.rs`) **no-ops it** — so a rendered "leveled"/"measured only" row
outcome (Summary or per-row UI state) is **not UI-observable offline**, even though the
underlying command completes and persists. This is a **deliberate, permanent seam** — not a
harness gap to close opportunistically — until someone bridges the Channel offline. The
**sanctioned observation path** for an offline spec that needs to prove such an outcome is the
same command over a raw `invoke()` (reading its RETURN value directly) plus the SimDevice event
log (`/sim/events`, offline-only) for the wire write — never a hand-rolled re-derivation through
the rendered UI. The Playwright specs that exercise this path exist to prove the HTTP bridge +
command-registration layer works end to end, as the **twin** of the Rust-only gates
(`e2e_server_tests.rs`) that prove the underlying physics/outcome — not to duplicate what the
Rust gate already covers.

Every other site that needs this fact states it in ONE line and points here — see
`level-setup.spec.ts`'s and `level-defaults.spec.ts`'s file headers, `e2e_server_tests.rs`'s
scene-leveling test doc comments, and `e2e/fixtures/COVERAGE.md` rows 6/20.

## Seeding and list reads

- Seed-path list reads are **TOLERANT plus a completeness floor, never `list_my_presets_strict`**. Strict decodes only terminal-frame streams and fails or garbles on back-to-back lean sessions (HW: tolerant returned 504/504 while strict returned truncated 190–236 fallbacks), and its re-arm retries themselves arm the HID open lockout.
- Online seeding runs a **FRESH `probe --seed-scenario` process BEFORE the server starts**, dodging the in-process `0xe00002c5` open lockout that aborted in-spec seeds. The seed self-repairs by sweeping stray imports — an aborted seed strands copies at the first empty slot anywhere in the bank.
- The ten scenario presets live in the scratch zone at list indices 400–409 and **stay resident between runs by default** — the pristine-checking seed re-imports any drifted or stale-rev slot. Teardown unconditionally disables re-amp, sweeps strays and recalls preset 001, but clears the scenario slots **only** with `TMP_E2E_CLEAR_SCENARIO=1`, for an on-demand net-zero run. **Their shapes are deliberate and the per-use-case map is [`e2e/fixtures/COVERAGE.md`](../../e2e/fixtures/COVERAGE.md)** — read it before changing a fixture, and update it in the same commit. In brief:
  - `E2E Rig` (400) — the scene-overlay + footswitch + Doctor-damage fixture.
  - `E2E Pedalboard` (401) — the scene-free copy/import + EXP/link-group fixture.
  - `E2E Edge` (402) — the split-output 8-scene fixture; also carries the Doctor's baked 2.6 kHz EQ-ring oracle.
  - `E2E Parallel` (403) — the both-lane-amps joint-k fixture.
  - `E2E Hiwatt 3S` (404) — a **verbatim device export** backing the wipe/bake/measurement-context gates (its exact byte length is pinned — do not edit it).
  - `E2E Preset24` (405) — the stale-load / saturated-pedal footswitch fixture (`level-fs-preset24.spec.ts`).
  - `E2E Combined Level` (406) — the new-flow leveling fixture (FS-alone / scene-alone "BASE SCENE" / scene-that-enables-an-FS, parallel both-amps-active, a post-cab compressor).
  - `E2E Doctor Oracle` (407) — 14 mixed-shape footswitches, one per Doctor spectral check, all bypassed in base.
  - `E2E Preset24 Min` (408) / `E2E Hiwatt Min` (409) — the smallest presets still reproducing each incident's own bug class.
- **THE CAB RULE (standing user directive):** every guitar amp in every fixture is a combo, an amp+cab-merged model (a cab/IR-suffixed id), or a bare head with a cabinet block DOWNSTREAM IN ITS OWN LANE. Enforced by `fixture_gates::every_guitar_amp_in_every_fixture_reaches_a_cab`, which walks the production routing decoder's signal paths — so a trunk head with a cab in each parallel lane passes and a lane-less cab does not. `E2E Preset24` used to be the exception (four drives into a naked `ACD_TwinReverb65NoFx`); P4-A appended a cab to it. Its `scenario-loudness.json` `"405"` C was deliberately LEFT AT `-21`: offline the C table _is_ the model, so keeping it preserves every committed pedal-curve outcome; the real unit's own loudness is measured, never read from the table.
- **Fixtures are generated, not hand-edited in place, and every regen bumps `FIXTURE_SOURCE_STAMP`'s `#rN` suffix** (`probe_api/seed_scenario.rs`) — a resident copy of an older rev must fail the pristine check and self-migrate. A regen also means rerunning `cargo test build_scenario_fixture -- --ignored` to rebuild `backup-fixture.bin`.

## `scripts/hw-e2e.sh` — the attended on-device layer

Runs the full Level + Copy happy paths against the real unit **non-destructively** (dry `--levelpreset` with no save, `--replace-held` with no commit, `--device-backup` read, `--reamp-off`). Override its `LEVEL_SLOT` / `COPY_*` env vars per unit. It is **attended, not a CI gate**, and acquires the machine-global device lock like the online `e2e.sh` path.

## External validation (P5) — ffmpeg is the only trusted meter here

`scripts/level-validate.sh` judges a leveled-and-saved sound's captured WAV with ffmpeg's `ebur128` filter against the TARGET the leveling run recorded — **never** against that run's own `predicted_lufs`/`verify_lufs`. The point is a meter this repo did not write: a `lufs.rs` regression that fools the Rust unit tests would still fool a self-consistency check, but not an independent BS.1770 implementation.

**Gate semantics (the decided answer — stated identically in `scripts/e2e.sh`'s header):**

- **ffmpeg ABSENT ⇒ advisory.** A loud, VISIBLE skip banner in the lane summary. Never a silent pass, never red either — ffmpeg is not a build or CI dependency of this app.
- **ffmpeg PRESENT and rows were emitted ⇒ a REAL GATE.** A target miss flips the suite red (`fail=1`).
- **ffmpeg present, no rows ⇒** nothing was strict-re-measured this run; logged and passed over.

**What runs when:**

- **`scripts/e2e.sh online …`** — exports `TMP_E2E_VALIDATE_LOG` + `TMP_E2E_VALIDATE_WAV_DIR` into the **e2e_server's** environment (that process is what runs the strict re-measures), then after the spec loop runs `level-validate.sh --expectations <log>` over the WAVs the server already dumped. There is deliberately **no post-suite re-capture loop**: re-driving `probe` afterwards opens fresh device sessions inside the 45–100 s lazy-save-commit window from a process whose `SLOT_SAVE_REGISTRY` is empty (`danger.md`), so it would read PRE-save bytes and fail correct runs. `TMP_E2E_VALIDATE_MAX_ROWS` (default 40) caps the pass and prints how many rows were dropped.
- **`scripts/validate-hbe.sh <preset-file>`** — the attended, standalone Friedman-HBE run: import → level (base + optional scenes/footswitches) → **wait out the commit window (150 s)** → re-measure with `--target`/`--dump-wav` → **completeness check** → `level-validate.sh --expectations` → clear, with a trap-guaranteed re-amp OFF (gap, one retry after a longer quiet, then a loud `probe --reamp-off` banner) on every exit path.
  The **completeness check is not optional bookkeeping**: a re-measure that dies before its capture emits NO row, and the judge can only grade rows it is handed — a shorter log is invisible to it. The script therefore records the label of every row it ASKS for and greps the log for each by identity (not by count, which a duplicate would mask) before calling the judge; a missing row is exit 1 whatever the judge said, including when the judge skipped for want of ffmpeg.
- **`scripts/level-validate.sh`** is the shared judge both callers use. `--expectations <jsonl>` (batch, the automated path), `--wav <path>` (one file, with `--probe-log` for the FLOOR/SILENT proof), or `--live <seconds>` (bare avfoundation capture — ATTENDED ONLY: it has no engage-proof, so the caller must verify engagement by other means).

**The emission seam:** `crate::validate_log`, appended to from the **measurement** seams — `leveller::measure_sound_asis_strict` (driven online by `e2e_measure_sound`) and `probe_api::level::probe_measure_current_lufs` (`probe --measure-scene --target … --dump-wav`). Not from the leveling run: the solve captures at its REFERENCE level, so its PCM is not the saved preset's output. Each row is written by the same capture that produced the run's own number, so it carries the WAV path, the engage verdict, and the sound's own **identity** (`slot` + `scene_slot`/`switch`) — never a position. That is why `LevelResult` now carries `scene_slot`: `level_scenes_apply_batched` filters failed scenes out of the array it returns, so index _i_ is not scene _i_ once anything fails.

Footswitch rows are now externally validated too — `probe --measure-footswitch <slot> <switch> <topology> [--lev g:n:p] [--target L] [--dump-wav D]` closed that hole. The **`doctor_apply` path is still NOT externally validated** (no expectation-emitting re-measure exists for it).

**Env vars:**

- `TMP_E2E_VALIDATE_LOG` — jsonl expectations path; `e2e.sh` sets it when ffmpeg is present. UNSET means every emission seam is a no-op.
- `TMP_E2E_VALIDATE_WAV_DIR` — WAV dump dir; defaults to a `level-validate-wavs` sibling of the log.
- `TMP_E2E_VALIDATE_MAX_ROWS` — row cap for the e2e pass, default 40.
- `TMP_E2E_LEVEL_TOL_LU` — validation tolerance in LU, default **1.0**. It must exceed the solver's own acceptance band of 0.3 LU plus recapture noise, or correct runs fail.
- `TMP_E2E_AVF_DEVICE` — avfoundation device id for `--live`, default `:0`.

**`level-validate.sh` exit codes, which both callers branch explicitly:** `0` every row passed (at least one actually measured) · `1` at least one row failed · `2` usage error · `3` ffmpeg absent, nothing checked · `4` **vacuous** pass — zero measured rows (every row clamped or persist-mismatched), announced in a yellow `PASS (VACUOUS)` banner. A `3` must be reported as SKIPPED, never as a target miss. A `4` is not a failure, but it must never certify: the online lane treats it as "passed but NOT stamped" — the zero-rows case is exactly the lazy-commit persist-regression shape the external judge exists to catch, so a skip is a real verdict, but it is not verification.

## Fixtures

- The offline `backup-fixture.bin` and `scenario-presets.json` **must stay in sync** — regenerate both from one script.
- **Fixture drift-lock trap:** a drift-lock or round-trip test that compares fixtures **through a typed struct** silently covers only the fields that struct carries. `info.product_id` and `info.preset_id` drifted while the lock test stayed green. Assert fixture invariants against the **RAW JSON** (`lib.rs`'s `fixture_gates`, deliberately OUTSIDE `#[cfg(feature = "e2e")]` — a gate that only compiles in a build nobody makes is not a gate). It pins `product_id == "tmStomp"` (a `"pro"` preset is rejected on the unit as "created using a newer firmware revision") and a unique `preset_id` per fixture preset.
- The preset XOR key is committed as `const PRESET_XOR_KEY: [u8;3] = *b"JLD"` in `backup.rs`. The runtime `derive_key`/`learn_key`/panic recovery was deleted — **do NOT reintroduce it**.

## Ports

`scripts/e2e.sh` derives a stable per-worktree bridge/vite pair (offset = `cksum(worktree-path) % 200`) and exports `TMP_E2E_PORT` / `TMP_E2E_VITE_PORT` / `TMP_E2E_WORKERS`.

**Offline claims a RANGE, not one port.** Each Playwright worker runs its own `e2e_server` on `TMP_E2E_PORT + parallelIndex` (`e2e/fixtures/port.ts` reads `TEST_PARALLEL_INDEX`), so the bridge base is `7800 + offset*8` — a fixed stride of 8 per worktree. Two properties are load-bearing and worth preserving:

- **The base is 7800, above the 7600-7799 window the old single-port scheme used.** Disjoint bands mean a worktree still running the old script cannot collide with a strided one during the migration. Two concurrent offline runs must never interfere.
- **`kill_port` filters by process name** (`ps -o comm=` matching `e2e_server`) before killing. `lsof -i tcp:N` matches either endpoint and cannot tell whose server it found; sweeping 8 ports unfiltered could kill a sibling worktree's live run or an unrelated dev server.

Still a `% 200` hash with no occupied-port retry, so two worktree paths can collide — it reduces contention, it does not guarantee isolation. The one real device is serialized by a machine-global `mkdir` lock (`scripts/device-lock.sh`).
