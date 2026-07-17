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
  (`DoctorOp`s) + the scene-loudness consistency check + the cut-through
  estimate.
- Device work — `leveller::doctor_capture` (capture) + `commands/doctor.rs`
  (`doctor_check` / `doctor_apply` / `doctor_save` / `doctor_discard` /
  `doctor_cancel`).

## The metric (deviation vs authored target + consensus)

Each capture's band powers (one shared post-onset body Welch PSD,
`doctor::body_psd`) become `deviation[i] = band_db[i] − TARGET[i]` against an
**authored per-family target curve** (`GUITAR_TARGET` = the median of the
25-slot flagship factory sweep through the humbucker stimulus; bass/bass-vi
targets are provisional few-anchor curves). Band powers are WIDTH-INTEGRATED
(a per-Hz-density-minded target misread every factory preset as bright+lost).
The deviations split via a **Theil–Sen** (median-slope) fit into a broadband
TILT (the dark/bright/thin lean) and per-band LOCALS — and every band rule
(muddy/boomy/harsh/lost/thin/buried) fires only on a **two-space consensus**:
the tilt-split local AND the median-centered deviation
(`centered_deviations`) must each clear their own gate; severity is the
smaller margin. Why two spaces: HW defect injection (`probe --doctor-inject`)
showed the tilt-split alone misattributes a skirted single-band defect (+12 dB
@250 Hz read as false "thin"), while the centered space alone is contaminated
by healthy tilt at the endpoint bands — the false-positive control lives in
the intersection. The metric is fully DETERMINISTIC per sound: a verdict
depends only on that sound's own measurements, never on which other sounds ran
in the same check.

## Diagnoses (13)

- **Band rules** (consensus, above): muddy / boomy / harsh / lost / thin /
  buried.
- **Tilt rules**: dark / bright (Theil–Sen slope vs the tilt gates).
- **fizzy** — self-relative (Air vs own presence band; the Air band is bimodal
  across a library, so a target-relative rule flagged every open preset),
  gated on top-octave spectral flatness under `StimulusKind::Capture` (noise
  hash fires, a bright cab's harmonic top doesn't).
- **washed** — post-stimulus tail-RMS rule (onset-aligned split).
- **spiky** — dynamics-spread rule (clean chains only).
- **resonant / boxy** — ENABLED since the 2026-07-17 parametric-EQ
  ground-truth round (`LOCALIZED_RULES_ENABLED = true`). Lineage: three
  earlier shape-only HW rounds (raw capture space → transfer space →
  Q-window + band corroboration) could not place gates, so the verdicts
  first shipped disabled. The ground-truth round dumped the
  `ACD_FiveBandParamEQ` schema (`filterN{frequency,gaindb,q}` are live
  changeParameter controlIds, HW-verified: 1 kHz/+12 dB/Q8 in → 1008 Hz/q 7.5
  measured; band 1 defaults to a high-pass — never use it for a peak), ran a
  height×Q injection matrix (+6/+9/+12 dB × Q 2–14 at 400–5600 Hz) through a
  real drives→'65 Deluxe+CabIR chain, and placed the gates against the
  25-slot factory peak population. Detection runs in TRANSFER space
  (`Psd::transfer_db` = capture log-PSD − STIMULUS log-PSD, stimulus ridges
  cancel; excess over a one-octave median envelope; peaks on
  `SoundProfile.peaks`). Gates: `resonant` = the strongest peak ≤ 4 kHz
  (above ≈4 kHz the cab-IR comb forest owns the transfer — 55/75 factory
  peaks, ungateable) with height ≥ 13.5 dB (factory in-range max: 12.6; the
  envelope saturates measured excess near 20·log10(Q/2), so the floor
  effectively targets Q ≳ 10 flagrant rings — feedback/parametric class),
  measured Q in [2, 40] (the estimator INFLATES q for strong on-chain rings
  — an injected Q14 flagrant ring measured h 24.2 / q 25.4; the ceiling only
  guards the isolated comb-needle class at q 85–455), AND band corroboration
  (the peak's OWN band hot in both consensus spaces, local AND centered >
  `RESONANT_MIN_BAND_LOCAL_DB` = 2.0). `boxy` = a 300–500 Hz hump ≥ 7.5 dB
  (a clean-site +12 dB Q8 hump measures 8.3; the factory bank has ZERO peaks
  below 662 Hz) under the same Q ceiling + corroboration, winning over
  resonant for the same peak. Scope honesty: a cocked WAH is a WIDE hump —
  peak-space h ≈ 6.7 while its mid local reads +15.7 dB — so wahs surface
  via the BAND rules (thin + the mid meter) by physics; the defects suite
  pins this (`resonant_wah` must NOT fire resonant) plus calibrated
  positives (`resonant_peq`: stacked 2×12 dB Q14 @ 2.6 kHz → resonant;
  `boxy_peq`: stacked 2×12 dB Q8 @ 420 Hz → boxy; 7/7 HIT). Q is measured
  pessimistically as max(±20-bin parabola-fit bandwidth, above-floor run
  width) — a first-crossing −3 dB walk reads estimate noise, not bandwidth.
  An octave-wide graphic-EQ lift keeps Q < 2 and never reads resonant
  (HW-verified). Their Rx is a cut on the log-nearest EQ-10 band naming the
  MEASURED frequency — generated inline (not via `generate_rx`, whose
  key-only signature can't carry the peak).

Thresholds are constants in `doctor.rs`, ONE table per family — Guitar /
Bass / **BassVi** (7-band layout with a (30,60) Sub band — measured and
displayed only, no rule keys on it yet); `StimulusKind` no longer selects a
table (Capture differs only via the fizzy flatness gate). The tables are
HW-calibrated (`notes/doctor-calibration.md`); recalibration edits values
there and nowhere else. A rule whose primary band was never excited is
skipped — **coverage keys on the CAPTURED OUTPUT's own SNR**
(`output_coverage_with_body`: band power vs the pre-onset noise floor +
`OUTPUT_SNR_MARGIN_DB`), not the input stimulus, so amp-created HF
(clipping harmonics, fizz) stays diagnosable on a dark DI input.

## Capture

Isolation matches leveling (Base = all block-acting footswitches off; a
footswitch sound gets its switch-active state; scenes ride their own
overrides) but derives **OFFLINE** from the startup backup scan
(`footswitch::derived_force_bypass` over `DoctorInput.{nodes,footswitches}`;
HW 60/60 equivalent to the old ~1.9 s per-preset field-8 read, which survives
only as the empty-graph fallback). One `resolve_sound_isolation` policy is
shared by `doctor_check` AND `doctor_apply`, so the A/B can never observe a
different bypass state than the diagnosis.

Window: a **3 s stimulus slice + 200 ms silent preamble pad**
(`doctor_stim_slice`) + a **graph-aware tail** — 1.5 s when the chain carries
a time-based block (the wash rule needs the decay), 0.3 s dry
(`doctor::doctor_tail_ms`). HW-A/B'd against the original 6 s + 2.5 s oracle
(`probe --doctor-window-ab`): 0 verdict flips. Capture runs ~4.7 s/sound
(pad + tail included). Captures are the **stereo mix** of USB-out 1/2
(`Capture::stereo_mix`, Doctor seams only — leveling's loudest-channel pick is
untouched, A/B'd −0.00 LU).

The body/tail split is onset-aligned: `audio::estimate_onset` (correlation
≥ 0.15 at a plausible ≤ 120 ms lag — recalibrated from HW latency
measurements, true latency 30–34 ms) locates the stimulus in the capture; the
pad's silence→signal edge makes onsets reliably confident on real chains, and
`doctor_signal_start` shifts the body PSD past the pad. Dry-chain tails
measure −21..−24 dB truer post-fix. The stimulus is profile-aware
(`resolve_stimulus_with_capture`): a calibrated profile's Tier-2 DI capture is
injected and diagnosed against the **CAPTURE threshold table** (a real DI
shifts band balance systematically, HW: +8..12 dB Lows / −8..10 dB Highs);
uncalibrated profiles use the synthetic topology WAV against the Synthetic
table. The capture tables currently equal the synthetic ones, so verdicts read
byte-identically until the DI sweep retunes capture space.

Capture choreography (`notes/perf.md`): consecutive **scene** sounds of the
same preset skip the per-sound preset reload (`doctor_skip_load` — only when
the previous sound wrote nothing and succeeded; base/footswitch sounds always
reload), and the capture connections use the lean handshake
(`Session::connect_lean`). Every capture is floor-guarded (a silent-inject
floor read retries once after a quiet gap). A single capture can occasionally
misread — repeated runs are the arbiter.

## Prescriptions & apply

`Rx` derivation is graph-aware (`graph_facts`): fixes prefer an existing
carrier block over inserting one, inserts are gated by the `blockcaps` limits,
and comp-aware rules avoid stacking compressors; parallel-split placements the
wire can't express are skipped. The **EQ move** (`eq_move`) is EQ-aware in
three tiers: (1) a drivable EQ-10 stereo already in the chain → a value-aware
one-click on it; (2) a DIFFERENT EQ already present (`OTHER_EQ_IDS`) → an
**advisory** to use the one you have; (3) no EQ → insert an EQ-10 anchored
right **after the cab** (`after_cab_anchor` — the wire anchor is the
neighbour's FenderId, NOT its node_id; a duplicated-model neighbour used to
silently drop the anchor). Param one-clicks are **value-aware** wherever the
current value rides the graph allowlist: a write that would move a known value
the WRONG way is dropped, and a blind write on an UNKNOWN value keeps an
honest "Set …" title. Apply (`doctor_apply`) edits the device edit buffer
under the DIAGNOSED scene/footswitch isolation — nothing persists until
`doctor_save`, which **rebuilds SAVED+ops from scratch** (restore → fresh
confirmed session → re-apply exactly `ops` → save), so intermediate
edit-buffer pollution can never be persisted structurally. `doctor_discard`
reloads the stored preset. The frontend serializes applies (`applyLock.ts`)
and allows ONE unsaved prescription at a time; A/B audition captures
before/after clips (BEFORE cached per sound — `BEFORE_CACHE`, keyed on
list index + name + stimulus + calibration + scene + footswitch; a cache hit
still reloads the slot). `severity.ts` ranks findings per sound and rolls up
the preset's worst severity.

## Cut-through estimate + reference match (flagship)

- **Cut-through** (`doctor::cut_through` → `DoctorSoundResult.cutThrough`):
  presence contrast = high-mids+highs over lows..mids band power (dB; Air and
  the Bass VI Sub band excluded), with a percentile against the **pinned
  25-value factory-bank distribution** (`FACTORY_CONTRAST_DB`) and an advisory
  below the factory p10 (+10 dB). Framed "estimated" in the UI
  (`CutThroughCard`); guitar-anchored — bass families report contrast only
  (no bass factory sweep yet). An ESTIMATE card, not a diagnosis: no rule, no
  Rx ops. (The originally-planned invented "dense mix masker spectrum" was
  dropped — anchoring on a measured distribution is defensible; a constant we
  made up is not.)
- **Reference match** (`matchModel.ts` + `MatchCard`, fully CLIENT-side): pick
  any sound of the run as reference; other same-layout sounds get EQ-10 moves
  from the balanceDb deltas already on the wire (log-nearest band, combined
  when bands collide, clamped ±6 dB, sub-1.5 dB moves dropped) applied through
  the existing PrescriptionCard A/B flow as one EQ-10 insert; when any delta
  exceeds EQ reach the card adds an honest cab/amp-swap advisory. Zero new
  captures, commands, or wire fields.

## Validation arms (probe)

- `probe --doctor-inject <slot> <gains_csv|none>` — single EQ-10 defect
  injection, before/after verdicts (the R5 consensus calibration evidence;
  also HW-verifies EQ-10 band controlIds — a wrong id shows as the defect not
  appearing).
- `probe --doctor-defects <slot> [--out r.json]` — the VERSIONED defect-recipe
  sweep (control / muddy / lost / washed / resonant_wah / resonant_peq /
  boxy_peq): each recipe's ops inject live, HIT/MISS/VIOLATION table against
  `must_fire`/`must_not_fire`. The recipe table in `doctor_defects.rs` is the
  fixture set.
- `probe --doctor-window-ab` / `--doctor-calib` — window re-baseline vs the
  pinned 6 s oracle · threshold-space sweeps. (The one-shot `--doctor-iso-ab`
  equivalence arm was retired once the offline/live isolation equivalence got
  its hardware-free pin in `commands/doctor_tests.rs`.)

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
the measured deviation), so this is free: one ~5 s hardware capture per sound,
then three microsecond-scale pure passes over ONE `RuleMetrics`
(`doctor::diagnose_levels` → `apply_thresholds` ×3; `doctor::playback_offsets`:
**Stage** tightens boomy/muddy −2.0 dB, fizzy −1.0 dB → fire earlier; **Quiet**
relaxes +2.0 / +1.0; **Rehearsal** is the anchor, 0 and byte-identical to the
legacy `diagnose()`) merged by diagnosis key. The offsets are **monotonic in
loudness** (louder ⇒ tighter ⇒ strictly more firings — asserted by
`playback_offsets_are_monotonic`), so a finding's firing set is always a
louder-suffix and one ordinal fully describes it: `LeveledDiag.from_level` ∈
{quiet, rehearsal, stage}. The UI renders this as `LevelIndicator`
(`src/views/doctor/LevelIndicator.tsx`) — three venue pictographs lit in the
finding's severity colour where it fires: `tiny` beside each diagnosis chip on
the collapsed triage row, `rich` (with labels) in the expanded header. `quiet`
shows as an all-lit "at any volume" state. **The indicator is a read-only render
of `from_level` and is fully decoupled from the Settings `playback_level`
store** — the Doctor diagnoses all three levels regardless of the room level
chosen for leveling. The offsets are additive at comparison time — they never
mutate the pinned `Thresholds` consts — and the table is **PROVISIONAL**,
pending an SPL-anchored recalibration sweep (see notes/doctor-calibration.md).
The marketing showcase's curated profiles sit far from every threshold, so they
tag `quiet` (untagged) at every level — the pinned `showcase_profile_diagnoses`
test uses the offset-free `diagnose()`.
