# Doctor (Doctor tab)

Feature-level doc; thresholds + calibration evidence live in
`notes/doctor-calibration.md`. The Doctor diagnoses selected sounds' tonal
problems from re-amp captures and offers one-click prescriptions.

## Flow

`Select → Setup → Run → Results` (`DoctorView` + `useDoctorFlow`; select is the
default stage). The RUN is READ-ONLY: `doctor_check` never saves, forces re-amp
OFF afterwards, and restores the active preset. Writes happen only from Results
via an explicit prescription apply.

## Engine split

- `doctor.rs` — PURE rules, no device I/O, no Tauri: capture measurements
  (`SoundProfile`) → diagnoses + graph-derived `Rx` prescriptions
  (`DoctorOp`s) + the scene-loudness consistency check.
- Device work — `leveller::doctor_capture` (capture) + `commands/doctor.rs`
  (`doctor_check` / `doctor_apply` / `doctor_save` / `doctor_discard` /
  `doctor_cancel`).

## Diagnoses (8)

muddy / boomy / harsh / fizzy / washed / lost / buried / **spiky** — band
deviations in "balance space" (a band's dB offset from the sound's own spectral
mean) judged against the cohort median when ≥ `MIN_COHORT` = 4 sounds ran, else
absolute neighbour expectations (the result carries `cohort: "median"|"absolute"`).
Exceptions: fizzy is self-relative (Air vs own presence band — the cohort median
is bimodal across a library); washed is a post-stimulus tail-RMS rule; spiky is a
dynamics-spread rule (clean chains only).

Thresholds are constants in `doctor.rs`, DUAL-keyed by **(Family, StimulusKind)**:
families Guitar / Bass / **BassVi** (Bass VI gets a 7-band layout with a (30,60) Sub
band for its E1 ≈ 41 Hz octave — measured and displayed only, no rule keys on it
yet), × Synthetic / Capture stimulus spaces. The synthetic tables are
HW-calibrated (`notes/doctor-calibration.md`); the `*_CAPTURE` tables are
provisional copies pending the attended `probe --doctor-calib` sweep.
Recalibration edits values there and nowhere else. A rule whose primary band the
stimulus never excited is skipped (per-sound band-coverage check).

## Capture

Same isolation rules as leveling (`doctor_force_bypass`: Base = all block-acting
footswitches off; a scene/footswitch sound gets its engaged state), but with a
**2.5 s tail** (`DOCTOR_TAIL_MS`) instead of leveling's 0.8 s — the wash rule
needs the decay. The stimulus is profile-aware (`resolve_stimulus_with_capture`):
a calibrated profile's Tier-2 DI capture is injected and the sound is diagnosed
in **Capture space** (`StimulusKind::Capture` — its own threshold table and
cohort key; capture and synthetic cohorts NEVER pool, a real DI shifts band
balance systematically, HW: +8..12 dB Lows / −8..10 dB Highs). Uncalibrated
profiles use the synthetic topology WAV in the HW-calibrated Synthetic space —
and since the capture tables currently equal the synthetic ones, their verdicts
read byte-identically until the `probe --doctor-calib` sweep retunes capture
space.

## Prescriptions & apply

`Rx` derivation is graph-aware (`graph_facts`): fixes prefer an existing
carrier block over inserting one, inserts are gated by the `blockcaps` limits,
and comp-aware rules avoid stacking compressors; parallel-split placements the
wire can't express are skipped. Apply (`doctor_apply`) edits the device edit
buffer on a held session — nothing persists until `doctor_save`;
`doctor_discard` reloads the stored preset. The frontend serializes applies
(`applyLock.ts`) and allows ONE unsaved prescription at a time; A/B audition
re-captures before/after clips for comparison. `severity.ts` ranks findings per
sound and rolls up the preset's worst severity (scene-jump bumps rank).

## Scene consistency

Separate from tonal rules: a scene whose loudness jumps ≥ `scene_delta_db` = 3 dB
from Base is flagged (advisory `SceneConsistency` + one-click `SceneTrim`) —
pointing at per-scene leveling rather than a tonal fix.
