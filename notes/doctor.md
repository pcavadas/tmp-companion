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
mean) judged against the cohort median when ≥ `MIN_COHORT` = 4 **presets** ran,
else absolute neighbour expectations (the result carries `cohort: "median"|"absolute"`).
The median populates from ONE representative sound per preset (its base sound
preferred, else its first measured sound) — a single preset's base + scenes would
otherwise be a degenerate cohort whose median ≈ the preset itself, self-normalizing
real problems away; every sound is still DIAGNOSED against that median.
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
needs the decay. The body/tail split is onset-aligned, not a fixed boundary:
`audio::estimate_onset` locates where the stimulus actually starts in the
capture (the buffer begins at stream start, before the audio propagates
through cpal/USB/DSP), and `tail_energy_ratio` splits body-vs-tail from that
onset — splitting at the raw stimulus length alone would leak latency-delayed
body signal into the tail and skew `washed` measurements (and any calibration
derived from them) toward the wash threshold. The stimulus is profile-aware (`resolve_stimulus_with_capture`):
a calibrated profile's Tier-2 DI capture is injected and the sound is diagnosed
in **Capture space** (`StimulusKind::Capture` — its own threshold table and
cohort key; capture and synthetic cohorts NEVER pool, a real DI shifts band
balance systematically, HW: +8..12 dB Lows / −8..10 dB Highs). Uncalibrated
profiles use the synthetic topology WAV in the HW-calibrated Synthetic space —
and since the capture tables currently equal the synthetic ones, their verdicts
read byte-identically until the `probe --doctor-calib` sweep retunes capture
space.

Capture choreography (2026-07, `notes/perf.md`): consecutive **scene** sounds of the
same preset skip the per-sound preset reload (`doctor_skip_load` — only when the
previous sound wrote nothing and succeeded; base/footswitch sounds always reload),
and the capture connections use the lean handshake (`Session::connect_lean`). A
single capture can occasionally misread (~1-in-7 outliers observed: a 3 dB band
shift, a −80 dB empty-tail sentinel) — repeated runs are the arbiter.

## Prescriptions & apply

`Rx` derivation is graph-aware (`graph_facts`): fixes prefer an existing
carrier block over inserting one, inserts are gated by the `blockcaps` limits,
and comp-aware rules avoid stacking compressors; parallel-split placements the
wire can't express are skipped. Param one-clicks are **value-aware** wherever
the current value rides the graph allowlist (`session::GraphNode.params`: reverb
`mix`/`wetdrymix`, cab `hpf`/`lpf`, EQ-10 `gain*hz`): a write that would move a
known value the WRONG way is dropped (washed skips an already-low mix; the
boomy/fizzy cab cut skips an hpf already ≥ 90 Hz / lpf already ≤ 8 kHz and falls
back to the advisory), and a blind write on an UNKNOWN value keeps an honest
"Set …" title instead of a directional "Raise/Lower/Cut" promise. Apply (`doctor_apply`) edits the device edit
buffer on a held session — nothing persists until `doctor_save`;
`doctor_discard` reloads the stored preset. The frontend serializes applies
(`applyLock.ts`) and allows ONE unsaved prescription at a time; A/B audition
captures before/after clips for comparison. The BEFORE clip is cached across
consecutive applies on the same sound (`BEFORE_CACHE`, single entry, keyed on
list index + name + stimulus path + calibration; a cache hit still reloads the slot — the load feeds
`confirm_active` and discards stale edit buffers; invalidated at the `Session`
stored-preset mutation choke points + device detach — see `notes/perf.md`). `severity.ts` ranks findings per
sound and rolls up the preset's worst severity (scene-jump bumps rank).

## Scene consistency

Separate from tonal rules: a scene whose loudness jumps ≥ `scene_delta_db` = 3 dB
from Base is flagged as an **advisory-only** `SceneConsistency` — Doctor has no
in-app scene trim (the wire can't set a scene's loudness relative to Base, and
Level-tab leveling targets an absolute LUFS), so every branch (louder scene,
quieter scene, block-acting footswitch, the scene-0 USB anomaly) advises leveling
it from the Level tab rather than promising a one-click. `DoctorOp` carries no
`SceneTrim` variant.

## Playback level (Fletcher–Munson, PROVISIONAL)

Every sound is diagnosed at **all three** playback levels at once, and each
finding is tagged with the quietest level it fires at — there is **no picker**.
Equal-loudness contours flatten as SPL rises, so low-frequency (boomy/muddy) and
mildly-HF (fizzy) content is perceptually hotter at stage volume; a preset can
genuinely be clean quiet and boomy loud, and the Doctor **shows** that instead of
hiding it behind a mode toggle.

The capture is level-independent (the offset shifts the comparison THRESHOLD, not
the measured deviation), so this is free: one ~11 s hardware capture per sound,
then three microsecond-scale pure passes. `doctor::diagnose_levels` runs the pure
`diagnose_kind` at each level (`doctor::playback_offsets`: **Stage** tightens
boomy/muddy −2.0 dB, fizzy −1.0 dB → fire earlier; **Quiet** relaxes +2.0 / +1.0;
**Rehearsal** is the anchor, 0 and byte-identical to the legacy `diagnose()`) and
merges by diagnosis key. The offsets are **monotonic in loudness** (louder ⇒
tighter ⇒ strictly more firings — asserted by `playback_offsets_are_monotonic`),
so a finding's firing set is always a louder-suffix and one ordinal fully
describes it: `LeveledDiag.from_level` ∈ {quiet, rehearsal, stage}. The UI renders
`quiet` untagged ("a problem at any volume"), `rehearsal`/`stage` as "at
{level} volume and up". The offsets are additive at comparison time — they never
mutate the pinned `Thresholds` consts — and the table is **PROVISIONAL**, pending
an SPL-anchored recalibration sweep (see notes/doctor-calibration.md). The
`set_playback_level` store value is now Settings/leveling-only (it no longer
gates diagnosis). The marketing showcase's curated profiles sit far from every
threshold, so they tag `quiet` (untagged) at every level — the pinned
`showcase_profile_diagnoses` test uses the offset-free `diagnose()`.
