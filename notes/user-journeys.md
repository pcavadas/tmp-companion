# User journeys — coverage map + bug→gate registry

Living doc. A journey's coverage verdict is **FULL** (an automated gate exercises the real
outcome, not just a terminal-state smoke), **PARTIAL** (some layer covers it — often a mocked
Vitest path or a pure-function unit test — but the end-to-end behavior isn't gated), or **NONE**
(no automated gate; a regression here ships silently). Update this table in the same PR that adds
or removes a spec. For the full per-tab click-by-click scenario inventory (the L1–L14/P1–P10/
S1–S14/T1–T7/E1–E10/C1–C7/M-\* ids referenced below), see `notes/e2e-test-plan.md` — this doc is
the outcome-level summary, not a duplicate of that inventory.

**FULL is not always CI-enforced.** A row marked "FULL (online only)" runs solely in the attended
`scripts/e2e.sh online` lane (real hardware, never CI) — a regression there is caught only when
someone runs it, not blocked automatically the way an offline/CI gate is. Read the parenthetical
on every FULL row before assuming "blocked on every push."

## Level

| Journey                                                                      | Coverage                                                                                                                                                                          | Gate                                                                                                              |
| ---------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| Single-pass level, plain preset (Base only)                                  | FULL                                                                                                                                                                              | `e2e/specs/level.spec.ts`                                                                                         |
| Single-pass level, preset with scenes + footswitches                         | FULL                                                                                                                                                                              | `e2e/specs/level.spec.ts`                                                                                         |
| Consecutive re-run, base lane — no drift                                     | FULL (online: true skip-branch gate) / PARTIAL (offline: events-equality only, `previous_level` is structurally always `None` without a field-8 read model)                       | `e2e/specs/level-rerun.spec.ts`                                                                                   |
| Consecutive re-run, footswitch lane — no drift                               | FULL (online only; fixed by `switch_at_target`, PR #85)                                                                                                                           | `e2e/specs/level-rerun.spec.ts`                                                                                   |
| First-run shipped defaults → mass clamp                                      | FULL (offline, sidecar-authored ceilings)                                                                                                                                         | `e2e/specs/level-defaults.spec.ts`                                                                                |
| Re-level-clamped loop (clamped at a loud target → resolves at a quieter one) | FULL (offline)                                                                                                                                                                    | `e2e/specs/level-defaults.spec.ts`                                                                                |
| Mid-run one-item capture fault → skip-and-continue                           | FULL (offline)                                                                                                                                                                    | `e2e/specs/level-defaults.spec.ts`                                                                                |
| No-signal / off-branch summary + routing remediation banner                  | FULL (offline)                                                                                                                                                                    | `e2e/specs/level-defaults.spec.ts`                                                                                |
| Silent capture never writes a near-zero level (pedal-fiasco)                 | FULL (offline: silent-capture-writes-nothing half) / PARTIAL (the forced-bypass-then-reload-before-save half is Rust-unit-only, not e2e-drivable offline — needs a field-8 model) | `e2e/specs/pedal-fiasco.spec.ts` + `leveller.rs::write_footswitch_values` tests + `copy_apply_one` op-order tests |
| Stop mid-run, then Continue                                                  | PARTIAL (Vitest mocked `RunBody` only)                                                                                                                                            | none (e2e-test-plan.md M-L1)                                                                                      |
| Detach mid-run                                                               | NONE                                                                                                                                                                              | none (e2e-test-plan.md M-C1)                                                                                      |
| Setup apply-to-all instrument/target binding                                 | PARTIAL (Vitest only)                                                                                                                                                             | none                                                                                                              |
| Settings target reorder / slider commit                                      | NONE                                                                                                                                                                              | none (e2e-test-plan.md E4/E5)                                                                                     |
| "Even out parallel amps" joint-k rebalance (splitMix summation)              | NONE — deliberately excluded; a single-knob offline capture model can't represent amp summation, so asserting against it offline would be meaningless                             | none — online-only class, no gate yet                                                                             |
| Relaunch / webview reload mid-operation                                      | NONE                                                                                                                                                                              | none (e2e-test-plan.md C-adjacent, M-C3 covers reload only at idle)                                               |

## Cross-feature (shared startup backup scan)

| Journey                                                        | Coverage                                                                                                                                                                                 | Gate                                                                                     |
| -------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| Calibrate a profile → re-level uses the fresh capture stimulus | PARTIAL (command-level only — topology-fallback + repeat-run stability; the real staleness journey and the DI-capture path aren't modeled, no spec yet — see the test's own doc-comment) | `src-tauri/src/e2e_server_tests.rs::cross_feature_profile_relevel_resolves_and_no_crash` |
| Doctor apply/save → re-level sees the post-write graph         | NONE                                                                                                                                                                                     | none                                                                                     |
| Copy block-swap → Level sees the post-swap candidates          | NONE                                                                                                                                                                                     | none                                                                                     |

## Doctor

| Journey                             | Coverage                                 | Gate                                    |
| ----------------------------------- | ---------------------------------------- | --------------------------------------- |
| Select → run → results, happy path  | FULL                                     | `e2e/specs/doctor.spec.ts`              |
| Prescription apply / save / discard | FULL (online-only; skipped offline)      | `e2e/specs/doctor-apply.online.spec.ts` |
| Scene-loudness consistency check    | PARTIAL (pure-rule Rust unit tests only) | `src-tauri/src/doctor.rs` unit tests    |

## Copy

| Journey                                              | Coverage                     | Gate                         |
| ---------------------------------------------------- | ---------------------------- | ---------------------------- |
| Multi-op edit + multi-preset save + optimistic cache | FULL                         | `e2e/specs/copy.spec.ts`     |
| Back discards staged edits (known UX trap)           | NONE — documented, not gated | none (e2e-test-plan.md M-P1) |
| Partial save failure (one of N targets errors)       | NONE                         | none (e2e-test-plan.md M-P3) |

## Songs

| Journey                                   | Coverage | Gate                            |
| ----------------------------------------- | -------- | ------------------------------- |
| Song/setlist CRUD                         | FULL     | `e2e/specs/songs.spec.ts`       |
| Add songs to a setlist + reorder + remove | NONE     | none (e2e-test-plan.md S10/S12) |

## Reliability / safety invariants

| Journey                                                                                       | Coverage                                          | Gate                                                                       |
| --------------------------------------------------------------------------------------------- | ------------------------------------------------- | -------------------------------------------------------------------------- |
| Bounded live-capture ring under sustained pushes ("locked up my machine" class)               | FULL                                              | `src-tauri/src/audio.rs::ring_append_stays_bounded_under_sustained_pushes` |
| Guaranteed re-amp-OFF ≥ ON on every leveling/doctor run, checked before teardown's own rescue | FULL (standing assert on every level/doctor spec) | `e2e/fixtures/scenario.ts::expectReampBalanced` (`/reamp/counters`)        |
| Drift lock: `backup-fixture.bin` ↔ `scenario-presets.json` stay in sync                       | FULL                                              | `backup_read_tests.rs` (CI-enforced)                                       |

## Bug→gate registry

One row per identified bug **class** (not necessarily the user's exact preset/steps) — the
enforcement anchor for "every user-reported bug enters the harness." A row with no gate yet is an
open item, not a gap to paper over.

| Date                                                          | Report                                                                                                                                                           | Bug class                                         | Gate                                                                                                                                                                                   | Fixture                                                          |
| ------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------- |
| 2026-07                                                       | Real user's first session: presets clamped far below the shipped default targets                                                                                 | Default-target mass clamp                         | `e2e/specs/level-defaults.spec.ts` ("whole-preset defaults → base clamps at its ceiling")                                                                                              | slot 403 "E2E Realistic" + `e2e/fixtures/scenario-loudness.json` |
| 2026-07                                                       | Same report: re-leveling a clamped item kept clamping / looked stuck                                                                                             | Re-clamp loop                                     | `e2e/specs/level-rerun.spec.ts` (events-equality) + `level-defaults.spec.ts` ("re-level-clamped: clamped at Lead resolves at Crunch")                                                  | slot 401 "E2E Target 1"                                          |
| 2026-07                                                       | Same report: a parallel-cab preset's mix-cut scene behaved oddly under leveling                                                                                  | splitMix structural clamp                         | `src-tauri/src/e2e_server_tests.rs::level_defaults_403_scenes_solve_and_offbranch` (command-level; the UI can't render per-scene outcomes offline — see level-defaults.spec.ts header) | slot 403 gtrParallel1                                            |
| 2026-07                                                       | Same report: "it re-levels items it already leveled" / some items just fail                                                                                      | Items failing / no-signal                         | `level-defaults.spec.ts` (mid-run capture-fault) + `pedal-fiasco.spec.ts` (off-branch → zero writes)                                                                                   | slot 402 capture-fault via `/sim/fault`                          |
| 2026-07                                                       | Same report, mirroring an earlier "pedal fiasco" incident (old app version, PR #31/#56 class)                                                                    | Silent capture must never write a near-zero level | `e2e/specs/pedal-fiasco.spec.ts`                                                                                                                                                       | slot 401/402, capture-fault armed                                |
| 2026-07                                                       | Earlier live incident: a rapid re-amp benchmark loop locked up the whole Mac                                                                                     | Unbounded live-capture ring → OOM                 | `src-tauri/src/audio.rs::ring_append_stays_bounded_under_sustained_pushes`                                                                                                             | synthetic sustained-push loop, no HW needed                      |
| 2026-07                                                       | Same first-session report: implied by the app's calibration feature interacting with leveling ("added a noodle on the Jaguar then everything changed")           | Calibration-shift not reflected in re-level       | PARTIAL — `src-tauri/src/e2e_server_tests.rs::cross_feature_profile_relevel_resolves_and_no_crash` (command-level; scope caveats in its own doc-comment — see the journey table above) | synthetic profile, no stored DI capture                          |
| PR #74 (2026-07-13), reopened by this session's investigation | The footswitch leveling lane re-solved and rewrote already-in-tolerance switches on every re-run — deferred as a follow-up in PR #74's own body and never landed | Footswitch idempotency regression                 | `e2e/specs/level-rerun.spec.ts` ("footswitch: run 2 rewrites nothing in-tolerance"); fixed by `switch_at_target` (`leveller.rs`, PR #85)                                               | slot 400 "E2E Reference" switch 1                                |
