# Doctor (Doctor tab)

Feature-level doc; thresholds + calibration evidence live in
`notes/doctor-calibration.md`. The Doctor diagnoses selected sounds' tonal
problems from re-amp captures and describes each fix as a step the player makes
on the unit. It never writes to the unit (the owner's decision): no
apply, no save, no before/after.

## Flow

`Select → Instrument → Check → Results` (`DoctorView` + `useDoctorFlow`; each
stage is a page under the toolbar, Select first). Instrument is one
per PRESET, remembered in the app's own store on this computer
(`set_preset_instrument` → `profile_by_slot`, never the unit); the page is
SKIPPED when every chosen preset already has one saved and still offered (an
explicit "None" counts), and the Steps then read "Check › Results". The CHECK is
READ-ONLY: `doctor_check` never saves, forces re-amp OFF afterwards, and restores
the active preset. Stop asks first ("Stop checking?"); after a Stop the Results
open by themselves. Results are one flat list worst first (`resultsModel.ts`:
the tone a row shows — danger for a confident high finding, caution for a medium
or borderline one — then slot, then order in the preset; a preset's "Scenes" and
"Footswitches" rows sort in by their own tone; then sounds that couldn't be
checked, then sounds never reached, then the clean ones folded into "N more sound
good"). The worst row is open on arrival; on a wide window the opened sound sits
in a panel beside the list. A failed sound is worded by its typed
`DoctorSoundResult.errorKind` (a `SkipKind`; `no_signal` refined by the preset's
backup-scan silence hint), never by its `error` text, which goes to the log. A
silent capture (integrated loudness −inf: a footswitch that routes to OUT2, an amp
at zero) is that `no_signal` kind (`audible_or_no_signal`), not an analysis error.

## Engine split

- `doctor.rs` — PURE rules, no device I/O, no Tauri: capture measurements
  (`SoundProfile`) → diagnoses + graph-derived `Rx` prescriptions
  (`DoctorOp`s) + the scene-loudness consistency check + the cut-through
  estimate.
- Device work — `leveller::doctor_capture` (capture) + `commands/doctor.rs`
  (`doctor_check` / `cancel_doctor_check`). Its `ops_session` /
  `apply_doctor_ops` (apply a fix's `DoctorOp`s live, never saved) serve only
  `probe --doctor-inject`; the app has no apply path.

## The metric (deviation vs authored target + consensus)

Each capture's band powers (one shared post-onset body Welch PSD,
`doctor::body_psd`) become `deviation[i] = band_db[i] − TARGET[i]` against an
**authored per-family target curve** (`GUITAR_TARGET` = the median shape of
the 248-sound guitar factory bank through the pitched-pluck humbucker
stimulus; bass/bass-vi targets are provisional few-anchor curves). Band powers are WIDTH-INTEGRATED
(a per-Hz-density-minded target misread every factory preset as bright+lost).
The deviations split via a **Theil–Sen** (median-slope) fit into a broadband
TILT (the dark/bright/thin lean) and per-band LOCALS — and every band rule
(muddy/boomy/harsh/lost/thin/buried) fires only on a **two-space consensus**:
the tilt-split local AND the median-centered deviation
(`centered_deviations`) must each clear their own gate; severity is the
smaller margin. Severity stays in the rule's own unit (dB, dB/oct) for
display; hedging and ranking read `Diag::z` = severity ÷ the rule's
run-to-run σ (`doctor::rule_sigma`: band rules and fizzy 0.4 dB, tilt
0.05 dB/oct, peak height 0.3 dB, tail ratio 0.3 dB), and the UI calls a
finding "Possible" below 2.5σ (`severity.ts::POSSIBLE_MAX_Z`; a band rule's
1.0 dB). Why two spaces: HW defect injection (`probe --doctor-inject`)
showed the tilt-split alone misattributes a skirted single-band defect (+12 dB
@250 Hz read as false "thin"), while the centered space alone is contaminated
by healthy tilt at the endpoint bands — the false-positive control lives in
the intersection. The centered space centers on the median of the COVERED
body bands only, so a band the capture never excited can't move a covered
band's verdict; with fewer than 3 covered body bands (`MIN_CENTERING_BANDS`)
it is unknown and every consensus rule (and the localized rules, which
corroborate in it) abstains. The metric is fully DETERMINISTIC per sound: a verdict
depends only on that sound's own measurements, never on which other sounds ran
in the same check.

### Re-deriving on a new stimulus

The Doctor's constants (`GUITAR_TARGET`, the `GUITAR` gates, `GUITAR_STIM_REF`,
`FACTORY_CONTRAST_DB`, `CUT_THROUGH_ADVISORY_DB`) come from a factory sweep
through the shipped pitched-pluck humbucker. To redo it after a stimulus
change:

1. `probe --doctor-calib-factory 0..254 --stim resources/samples/guitar-humbucker.wav --family guitar --out <report.json>` (~37 min, through `leveller::analyze_capture`). Keep the previous sweep: it is the baseline.
2. `probe --doctor-derive <report.json> --baseline <previous report.json>` prints the target (the bank's median shape), the contrast population, and the gates. Each gate is RANK-TRANSFERRED (`benchmark::transfer_gate`): it keeps the percentile the shipped gate held in the baseline population, so every rule's false-alarm rate on healthy presets carries over. A max-plus-margin re-derive over the whole bank widened `lost` by 21 dB and silenced the defect presets. Paste `GUITAR_STIM_REF` from `print_guitar_stim_ref`.
3. `probe --doctor-loo <report.json>` for the false-positive rate per rule against the baseline's, then the defects suite (all 7 recipes must hit) and the empty-slot capture (no finding).
4. `probe --doctor-replay <capture.wav,…> --stim <wav>` re-runs the analysis on raw captures offline, so a detector change can be checked without the unit.

The rank transfer keeps the deliberately sensitive in-sample end: about half
of the healthy guitar factory bank gets at least one finding, where a
whole-bank max-plus-margin derivation flags about a fifth, mostly `washed`,
but silences the defect presets (HW, factory sweep, fw 1.8.58). The `washed`
gate is −8.5 dB because the pitched stimulus lifts every tail ratio. Between
the stimulus's harmonic lines the capture holds the chain's own noise and
intermodulation, which read as narrow fake peaks, so peak detection reads the
transfer only where the stimulus reaches its own one-octave median
(`Psd::observable_transfer_db`). The `muddy`/`boomy` gates the Stage playback
level tightens are floored at 0 dB, so a band sitting on the target never
reads as excess.

The bass and Bass VI targets are provisional anchors (user slots 21 and 8–9)
and need re-deriving on the bass stimuli the same way.

## Diagnoses (12)

- **Band rules** (consensus, above): muddy / boomy / harsh / lost / thin /
  buried.
- **Tilt rules**: dark / bright (Theil–Sen slope vs the tilt gates).
- **fizzy** — self-relative (Air vs own presence band; the Air band is bimodal
  across a library, so a target-relative rule flagged every open preset),
  gated on top-octave spectral flatness under `StimulusKind::Capture` (noise
  hash fires, a bright cab's harmonic top doesn't). The metric is corrected
  for the stimulus like the band rules (`fizzy_metric`): the stimulus's
  own Air − Highs minus the humbucker reference's is subtracted, so the
  pickup chosen in Settings does not decide whether it fires. The
  cut-through contrast gets the same correction. Both are exact no-ops for
  the humbucker and for fixtures without stimulus bands.
- **washed** — post-stimulus tail-RMS rule (onset-aligned split, tail window
  pinned to `DOCTOR_TAIL_MS`). The split's onset comes from a
  floor-relative energy step first (`leveller::doctor_onset`), falling back to
  the envelope correlator only when the step doesn't fire — the correlator's
  own confidence gate can't detect a PEAKLESS correlation curve, which is what
  a wash/reverb chain actually produces (HW: flat 0.27–0.37 across every lag
  0–250 ms on a 65%-wet plate).
- **resonant / boxy** — localized peaks in TRANSFER space
  (`Psd::transfer_db` = capture log-PSD − stimulus log-PSD, excess over a
  one-octave median envelope; `SoundProfile.peaks`). `resonant` = the
  strongest peak ≤ 4 kHz (above it the cab-IR comb forest owns the transfer)
  with height ≥ 13.5 dB and measured Q in [2, 40]; `boxy` = a 300–500 Hz hump
  ≥ 7.5 dB under the same Q range, winning over resonant for the same peak.
  Both need band corroboration (the peak's own band hot in both consensus
  spaces, > `RESONANT_MIN_BAND_LOCAL_DB` = 2.0). Q is `f0 / (f_hi − f_lo)`
  between the half-height points, the RBJ peaking filter's own bandwidth
  definition. A cocked wah is a wide hump and surfaces through the band rules
  (`resonant_wah` must NOT fire resonant). The Rx (`localized_cut_rx`) is a
  parametric cut at the measured frequency — depth `min(height/2, 6 dB)` for
  resonant, 4 dB for boxy, the measured Q clamped to 2–12 — on a free band of
  an existing `ACD_FiveBandParamEQ`, an advisory when it has none, else an
  `ACD_FiveBandParamEQ` insert after the cab, falling back to the log-nearest
  EQ-10 band when the insert fails the caps. Gate evidence:
  `notes/doctor-calibration.md`. The defects suite, factory silence and the
  AFTER capture on the current estimator are not yet re-checked on the unit
  (open: notes/open-questions.md).

Thresholds are constants in `doctor.rs`, ONE table per family — Guitar /
Bass / **BassVi** (7-band layout with a (30,60) Sub band — measured and
displayed only, no rule keys on it yet); `StimulusKind` does not select a
table (Capture differs only via the fizzy flatness gate). The tables are
HW-calibrated (`notes/doctor-calibration.md`); recalibration edits values
there and nowhere else. A rule whose primary band was never excited is
skipped — **coverage is judged on the CAPTURED OUTPUT's own SNR**
(`output_coverage_with_body`: band power vs the pre-onset noise floor +
`OUTPUT_SNR_MARGIN_DB`), not the input stimulus, so amp-created HF
(clipping harmonics, fizz) stays diagnosable on a dark DI input.

## Capture

Isolation matches leveling for Base/footswitch sounds (Base = all
block-acting footswitches off; a footswitch sound gets its switch-active
state) and derives **OFFLINE** from the startup backup scan
(`footswitch::derived_force_bypass` over `DoctorInput.{nodes,footswitches}`;
HW-equivalent to a live `presetDataRequest` (field 8) read, which remains the
fallback when the backup scan has no graph). A **scene sound gets NO force-bypass write
at all** — `derived_force_bypass` is never called for one — because the scene
recall itself asserts whatever overlay/bypass state the scene SAVED
(as-played: a scene's diagnosis must hear what the player hears, not a
synthetic all-off baseline). One `resolve_sound_isolation` policy decides every
capture of a sound, so none can observe a different bypass state than the
diagnosis.

**One graph per sound.** The tail policy, the
diagnosis and the prescriptions' current values all read the graph the sound
was CAPTURED in (`SoundIsolation::graph`), never the saved base graph:
`doctor::effective_nodes` applies the sound's isolation writes
(base/footswitch: `SoundIsolation::overrides`, the exact lists the capture
sends) or its saved scene overlay (`BackupPresetRow.scene_overrides`, from
`scene_jobs::scene_node_overrides`) onto `DoctorInput.nodes`. A
switch-enabled reverb that is saved bypassed is therefore diagnosed wet.

Window: a **3 s stimulus slice + 200 ms silent preamble pad**
(`doctor_stim_slice`) + a **graph-aware tail** — 1.5 s when the chain carries
a time-based block (the wash rule needs the decay), 0.3 s dry
(`doctor::doctor_tail_ms`; HW: 0 verdict flips against a 6 s + 2.5 s window,
`probe --doctor-window-ab`). Capture runs ~4.7 s/sound
(pad + tail included). Every capture is analysed through ONE seam,
`leveller::analyze_capture`, which the production check and the
calibration sweeps all call, so the shipped constants are derived in exactly
the space they are applied in:

- **Stereo combines in power.** Band powers, air-flatness, localized
  peaks, the coverage floor and the tail ratio take USB-out 1/2 separately
  (`leveller::DoctorSignal`) and average their PSDs and RMS², not their
  waveforms, so a decorrelated stereo reverb tail keeps its full power and
  chorus or doublers do not comb-filter into notches the player never hears.
  The onset detector reads the per-sample average (`DoctorSignal::mix`). The reported
  `integrated_lufs` is the 2-ch BS.1770 SUM every other output-side
  measurement uses (`leveller::doctor_capture_with_loudness`).
- **The body excludes the tail and the pad.** The body PSD spans
  `[signal_start, body_end)` (`doctor::body_psd_span`), and the stimulus PSD
  spans the stimulus without its pad (`leveller::stim_body_psd`): same length,
  same Welch grid, so a wire chain's transfer is flat to <0.05 dB and a wet
  preset's decay does not mix into its band powers. The tail ratio's body RMS
  starts at the signal too.
- **Band edges are half-open** (`[lo, hi)`), so a 3 or 6 kHz edge that
  lands exactly on a bin counts once.

The body/tail split is onset-aligned: `leveller::doctor_onset` locates the
stimulus in the capture via a floor-relative energy step first, falling back
to `audio::estimate_onset` (correlation ≥ 0.15 at a plausible ≤ 120 ms lag;
HW latency 30–34 ms) only when
the step doesn't fire — see the `washed` bullet above for why the correlator
alone isn't trusted as primary. The pad's silence→signal edge makes the
energy step reliably confident on real chains, and `doctor_onset`'s
`signal_start` shifts the body PSD past the pad. The stimulus is profile-aware
(`resolve_stimulus_with_capture`): a calibrated profile's Tier-2 DI capture is
injected verbatim and diagnosed against the SAME per-family threshold table as
a synthetic stimulus (a real DI shifts band balance systematically, HW:
+8..12 dB Lows / −8..10 dB Highs; only the fizzy flatness gate branches on
`StimulusKind`).

Capture choreography (`notes/perf.md`): consecutive **scene** sounds of the
same preset skip the per-sound preset reload (`doctor_skip_load` — only when
the previous sound wrote nothing and succeeded; base/footswitch sounds always
reload), and the capture connections use the lean handshake
(`Session::connect_lean`). Every capture is checked before it is diagnosed
(`commands/doctor.rs`'s `capture_suspect`): a floor read (the momentary
floor guard), an un-aligned onset (neither the energy step nor the correlator
found the stimulus, the path a silent inject takes) or a capture that ends
before the body does (no tail window, which `tail_energy_ratio` would read as
its −80 dB "bone dry" sentinel) retries once after a quiet gap with a fresh
load, then reports the sound as an error, never a diagnosis. The coverage
gate's pre-onset floor averages 2048-point Welch segments
(`psd::welch_psd_floor`) and compares by mean band density, because one
8192-point segment scattered the narrow low bands' floor by 1.5–2.5 dB run to
run. The stimulus PSD is computed once per stimulus, not per capture. A
single capture can occasionally misread — repeated runs are the arbiter.

## Prescriptions

`Rx` derivation is graph-aware (`graph_facts`): fixes prefer an existing
carrier block over inserting one, inserts are gated by the `blockcaps` limits,
and comp-aware rules avoid stacking compressors; parallel-split placements the
wire can't express are skipped. The **EQ move** (`eq_move`) is EQ-aware in
three tiers: (1) a drivable EQ-10 stereo already in the chain → a value-aware
one-click on it; (2) a DIFFERENT EQ already present (`OTHER_EQ_IDS`) → an
**advisory** to use the one you have; (3) no EQ → insert an EQ-10 anchored
right **after the cab** (`after_cab_anchor` — the wire anchor is the
neighbour's node id, which the device looks up among the group's current node
ids; a FenderId anchor aborts the device server, see `danger.md`). Param one-clicks are **value-aware** wherever the
current value rides the graph allowlist: a write that would move a known value
the WRONG way is dropped, and a blind write on an UNKNOWN value says so in its
`Rx::detail`. The app shows each fix's `Rx::title` only, as a bullet under "What
to change", in the backend's order; the `ops` stay on the wire (the shared-block
line reads which blocks they touch) but the app never applies them (the
owner's decision).
The muddy and harsh EQ cuts grow with the finding's margin past its
gate (`sized_cut_db`: the shipped 3 dB / 2 dB plus the margin, capped at 6 dB),
never below the shipped size. `severity.ts` gives each finding the tone it shows
and the one worst-first order (`findingTone`, `sortedDiags`).

**SNR-gate transparency:** `DoctorSoundResult.skippedBandCount` counts the
bands the SNR gate (`output_coverage_with_body`) dropped as too quiet to trust
(0 when no coverage was computed, an errored or showcase sound); when it is
non-zero the opened sound says "Some bands were too quiet to check." under its
last finding rather than silently under-checking a quiet capture.

**Leveling-damage advisories (`doctor::leveling_damage_hints`):** a
BACKUP-SCAN-ONLY, zero-capture scan of a preset's footswitch `param`
assignments for two shapes the CURRENT `param_class` gate could never have
produced — a wet-mix param assign-leveled to ~0 (an effect silenced whenever
the switch engages) and an Other-class param actually swept (never a level
control). Surfaced per preset as an advisory-only "Footswitches" row (sibling of
the "Scenes" row, caution, "2 footswitches worth checking") — factual copy only
("Turns the effect almost silent when you press it."), never "damage", since a
footswitch can legitimately sweep any parameter via Pro Control. It sorts in
with the findings, so a preset whose ONLY finding is this advisory still shows.

## Cut-through estimate + reference match (flagship)

- **Cut-through** (`doctor::cut_through` → `DoctorSoundResult.cutThrough`):
  presence contrast = high-mids+highs over lows..mids band power (dB; Air and
  the Bass VI Sub band excluded), with a percentile against the **pinned
  248-value factory-bank distribution** (`FACTORY_CONTRAST_DB`) and an advisory
  below `CUT_THROUGH_ADVISORY_DB` = −6.2 dB. Shown in an opened sound as "Cutting through
  the mix" ("More presence than 62% of the factory presets."), and hidden when
  there is no factory comparison; guitar-anchored — bass families report
  contrast only (no bass factory sweep yet). An ESTIMATE card, not a diagnosis: no rule, no
  Rx ops.
- **Reference match** (`matchModel.ts` + `resultsModel.ts::matchAdvice`, fully
  CLIENT-side): "Use as reference" on any sound of the run; other same-layout
  sounds get "To sound like <reference>" with EQ-10 moves
  from the balanceDb deltas already on the wire, centred on the MEDIAN body-band
  delta so Air and Bass VI's Sub can't shift every other move through the
  all-band mean (log-nearest band, combined
  when bands collide, clamped ±6 dB, sub-1.5 dB moves dropped) worded as one
  step: on the chain's own 10-band EQ, on a new one after the cab, or — with no
  CPU room for one — on an EQ already there; when any delta exceeds EQ reach it
  adds "EQ only gets part of the way. A different cab or amp gets closer." Zero
  new captures, commands, or wire fields.

## Validation arms (probe)

- `probe --doctor-inject <slot> <gains_csv|none>` — single EQ-10 defect
  injection, before/after verdicts (the consensus calibration evidence;
  also HW-verifies EQ-10 band controlIds — a wrong id shows as the defect not
  appearing).
- `probe --doctor-defects <slot> [--out r.json]` — the VERSIONED defect-recipe
  sweep (control / muddy / lost / washed / resonant_wah / resonant_peq /
  boxy_peq): each recipe's ops inject live, HIT/MISS/VIOLATION table against
  `must_fire`/`must_not_fire`. The recipe table in `doctor_defects.rs` is the
  fixture set.
- `probe --doctor-window-ab` / `--doctor-calib` — window re-baseline vs the
  pinned 6 s oracle · threshold-space sweeps.

## Words on screen

`doctor.rs` writes every Doctor sentence the app shows: each finding's `label`
and `explain`, and each fix's `Rx::title`. They follow the app's voice: plain
guitarist's words, one idea per sentence, no dashes, semicolons or hedging
("may", "a touch"), no Hz figures or knob values, and the band names the band
meter shows (`LABELS_6`: Lows, Low mids, Mids, High mids, Highs, Air — display
only; a band's identity is its index, `Family::semantic_bands`). A finding's
name never carries a frequency ("Boxy", "Ringing"); where a fix has to say
where, it names the band ("cut a narrow notch in the high mids").

Fixes come **simplest first**: a knob on a block the preset already has (the
amp's, a cab's, an EQ's, a reverb's, a compressor's), then a block to add, and
every fix after the first starts with "Or". `knob_first` puts the amp-knob
step ahead of a cab-filter fix that has to insert a block; muddy, harsh, lost
and buried always lead with their knob step. The exact settings behind each
step (bands in Hz, dB, Q, the cab filter's frequency, whether a write is
relative or outright) ride `Rx::detail`, and the reading behind each finding
rides `Diag::detail`; neither is for the screen, and `doctor_check` logs both
(`log_findings`). `Diag::direction` (`strong` | `weak`) says whether the
finding's bands are too strong or too weak, for the band meter's words. The
voice tests in `doctor.rs` (`every_fix_reads_simplest_first_in_the_app_voice`,
`every_finding_names_itself_without_figures_and_says_which_way_its_bands_lean`,
`scene_advice_reads_in_the_app_voice`) pin all of this.

## Scene consistency

Separate from tonal rules: a scene whose loudness jumps ≥ `scene_delta_db` = 3 dB
from Base is flagged as an **advisory-only** `SceneConsistency` — Doctor has no
in-app scene trim (the wire can't set a scene's loudness relative to Base, and
Level-tab leveling targets an absolute LUFS), so every branch advises a step on
the unit rather than promising a one-click: a louder scene turns its amp's Level
down until the jump is within the gate (or raises the Middle to stand out
instead), a louder footswitch sound turns down the block it turns on, a quieter
one is turned up if it isn't on purpose, and the scene-0 USB anomaly asks for a
check by ear. The app adds the Level-tab step itself, except when the louder sound
is the first scene (wire scene 0): its reading isn't trusted (open:
notes/open-questions.md), so the opened row
says the reading isn't certain and adds no leveling step. `DoctorOp` carries no
`SceneTrim` variant.

The steps name each sound as the screen does: `DoctorInput.sound_name` ("Lead",
"footswitch 3"), which the frontend sends beside the wire `label`. The label
("Tweed Warm · Lead") also names the preset, so it stays in the rows and
`worst_name`, where the frontend finds the sound by it, and never in a sentence.

Only MEASURED sounds enter consistency. A Stop returns the sounds the run never
reached with `status: "unrun"` and `integratedLufs: null` — never a placeholder
reading — and the title never says "Nothing to change in these N sounds" (or "in
this sound") while one is unrun or errored.

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
(`doctor::diagnose_levels` → `apply_thresholds` ×3; `doctor::playback_offsets`,
derived from ISO 226:2023 at 60/70/80 phon: **Stage** tightens boomy −0.7 local /
−3.0 centered, muddy 0.0 / −1.4, fizzy −0.6 dB → fire earlier; **Quiet** mirrors
it; **Rehearsal** is the anchor, 0 and byte-identical to the offset-free `diagnose()`)
merged by diagnosis key. The offsets are **monotonic in loudness** (louder ⇒
tighter or equal ⇒ never fewer firings — asserted by
`playback_offsets_are_monotonic`), so a finding's firing set is always a
louder-suffix and one ordinal fully describes it: `LeveledDiag.from_level` ∈
{quiet, rehearsal, stage}. The UI renders this inside an opened sound only, in
each finding's heading (`FindingBlock.tsx`): "Heard at every volume" / "Heard at
rehearsal volume and up" / "Heard only at stage volume" beside three venue icons
(Quiet, Rehearsal, Stage) lit in the finding's tone where it fires. No row
carries them. **The indicator is a read-only render
of `from_level` and is fully decoupled from the Settings `playback_level`
store** — the Doctor diagnoses all three levels regardless of the room level
chosen for leveling. The offsets are additive at comparison time — they never
mutate the pinned `Thresholds` consts. Their Rehearsal anchor is still an
assumption until a sweep records monitor SPL (see notes/doctor-calibration.md).
The marketing showcase's curated profiles sit far from every threshold, so they
tag `quiet` (untagged) at every level — the pinned `showcase_profile_diagnoses`
test uses the offset-free `diagnose()`.
