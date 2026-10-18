# Doctor

Read this when changing the Doctor's flow, rules, prescriptions or on-screen words.

The Doctor diagnoses the tonal problems of selected sounds from re-amp captures and describes each fix as a step the player makes on the unit.
It never writes to the unit (the owner's decision): no apply, no save, no before/after.
The metric, its thresholds and their derivation are in [doctor-thresholds.md](doctor-thresholds.md).
Facts are for firmware 1.8.58 unless a line says otherwise.

## Flow

`Select → Instrument → Check → Results` (`DoctorView` + `useDoctorFlow`); each stage is a page under the toolbar, Select first.
The instrument is chosen once per preset and remembered in the app's own store on this computer (`set_preset_instrument` → `profile_by_slot`), never on the unit.
The Instrument page is skipped when every chosen preset already has one saved and still offered (an explicit "None" counts), and the steps then read "Check › Results".
The check is read-only: `doctor_check` never saves, forces re-amp OFF afterwards, and restores the active preset.
Stop asks first ("Stop checking?"); after a Stop the Results open by themselves.

Results are one flat list, worst first (`resultsModel.ts`): by the tone a row shows (danger for a confident high finding, caution for a medium or borderline one), then slot, then order in the preset.
A preset's "Scenes" and "Footswitches" rows sort in by their own tone; then come sounds that could not be checked, then sounds never reached, then the clean ones folded into "N more sound good".
The worst row is open on arrival; on a wide window the opened sound sits in a panel beside the list.
A failed sound is worded by its typed `DoctorSoundResult.errorKind` (a `SkipKind`; `no_signal` refined by the preset's backup-scan silence hint), never by its `error` text, which goes to the log.
A silent capture (integrated loudness −inf: a footswitch that routes to OUT2, an amp at zero) is that `no_signal` kind (`audible_or_no_signal`), not an analysis error.

## Engine split

- `doctor.rs` holds the pure rules, with no device I/O and no Tauri: capture measurements (`SoundProfile`) → diagnoses, graph-derived `Rx` prescriptions (`DoctorOp`s), the scene-loudness consistency check and the cut-through estimate.
- Device work is `leveller::doctor_capture` (capture) and `commands/doctor.rs` (`doctor_check`, `cancel_doctor_check`).
  Its `ops_session` / `apply_doctor_ops` (apply a fix's `DoctorOp`s live, never saved) serve only `probe --doctor-inject`; the app has no apply path.

## Diagnoses

Twelve diagnoses:

- **Band rules** (consensus): muddy, boomy, harsh, lost, thin, buried.
- **Tilt rules**: dark, bright (the Theil–Sen slope against the tilt gates).
- **fizzy**: self-relative (Air against the sound's own presence band; the Air band is bimodal across a library, so a target-relative rule flagged every open preset), gated on top-octave spectral flatness under `StimulusKind::Capture` (noise hash fires, a bright cab's harmonic top does not).
  `fizzy_metric` corrects for the stimulus like the band rules: the stimulus's own Air − Highs minus the humbucker reference's is subtracted, so the pickup chosen in Settings does not decide whether it fires.
  The cut-through contrast gets the same correction; both are exact no-ops for the humbucker and for fixtures without stimulus bands.
- **washed**: a post-stimulus tail-RMS rule with an onset-aligned split ([doctor-thresholds.md#onset-alignment](doctor-thresholds.md#onset-alignment)) and a tail window pinned to `DOCTOR_TAIL_MS`.
- **resonant** and **boxy**: localized peaks ([Localized rules](#localized-rules)).

There is no dynamics-spread (`spiky`) rule, for the reason the leveling run has no dynamics cause ([leveling-scenes.md#verify-by-ear-causes](leveling-scenes.md#verify-by-ear-causes)).

## Localized rules

`resonant` and `boxy` read peaks in transfer space (`Psd::transfer_db` = capture log-PSD − stimulus log-PSD, the excess over a one-octave median envelope; `SoundProfile.peaks`).
`resonant` is the strongest peak ≤ 4 kHz (above it the cab-IR comb forest owns the transfer) with height ≥ 13.5 dB and measured Q in [2, 40]; `boxy` is a 300–500 Hz hump ≥ 7.5 dB under the same Q range, and wins over resonant for the same peak.
Both need band corroboration: the peak's own band hot in both consensus spaces, above `RESONANT_MIN_BAND_LOCAL_DB` = 2.0.
Q is `f0 / (f_hi − f_lo)` between the half-height points, the RBJ peaking filter's own bandwidth definition.
Between the stimulus's harmonic lines the capture holds the chain's own noise and intermodulation, which read as narrow fake peaks, so peak detection reads the transfer only where the stimulus reaches its own one-octave median (`Psd::observable_transfer_db`).
A cocked wah is a wide hump: peak-space height ≈ 6.7 with a mid local of +15.7 dB, so it surfaces through the band rules, and the `resonant_wah` recipe must not fire resonant [HW].

The prescription (`localized_cut_rx`) is a parametric cut at the measured frequency: depth `min(height/2, 6 dB)` for resonant, 4 dB for boxy, the measured Q clamped to 2–12.
It goes on a free band of an existing `ACD_FiveBandParamEQ`, is an advisory when that EQ has no free band, and is otherwise an `ACD_FiveBandParamEQ` insert after the cab, falling back to the log-nearest EQ-10 band when the insert fails the caps.
On a clean preset with the shipped stimulus, `probe --doctor-defects` hits the control, muddy, lost, `resonant_peq` and `boxy_peq` recipes; `washed` reads `dark` and `resonant_wah` reads `harsh` and `bright` instead [HW].
The prescribed resonant cut (−6 dB, Q 12) clears the ring without an octave-wide dip; the boxy cut, capped at 4 dB, leaves the stacked +24 dB recipe still reading `boxy` [HW].

The gates and the device facts behind them are in [doctor-thresholds.md#localized-rule-gates](doctor-thresholds.md#localized-rule-gates).

## Capture

Isolation matches leveling for base and footswitch sounds: base has every block-acting footswitch off, and a footswitch sound gets its switch-active state.
It is derived offline from the startup backup scan (`footswitch::derived_force_bypass` over `DoctorInput.{nodes,footswitches}`), which is equivalent to a live `presetDataRequest` (field 8) read [HW]; that read stays the fallback when the backup scan has no graph.
A scene sound gets no force-bypass write at all, because the scene recall itself asserts the overlay and bypass state the scene saved: a scene's diagnosis must hear what the player hears.
One `resolve_sound_isolation` policy decides every capture of a sound, so none can observe a different bypass state than the diagnosis.

**One graph per sound.**
The tail policy, the diagnosis and the prescriptions' current values all read the graph the sound was captured in (`SoundIsolation::graph`), never the saved base graph.
`doctor::effective_nodes` applies the sound's isolation writes (base and footswitch: `SoundIsolation::overrides`, the exact lists the capture sends) or its saved scene overlay (`BackupPresetRow.scene_overrides`, from `scene_jobs::scene_node_overrides`) onto `DoctorInput.nodes`.
A switch-enabled reverb that is saved bypassed is therefore diagnosed wet.

**Window.**
A 3 s stimulus slice plus a 200 ms silent preamble pad (`doctor_stim_slice`, `leveller::DOCTOR_PAD_MS`) plus a graph-aware tail: 1.5 s when the chain carries a time-based block (the wash rule needs the decay), 0.3 s dry (`doctor::doctor_tail_ms`).
Against a 6 s + 2.5 s window it flipped no verdict (`probe --doctor-window-ab`) [HW].
A capture takes ~4.7 s per sound, pad and tail included.
Every capture is analysed through one seam, `leveller::analyze_capture`, which the production check and the calibration sweeps all call, so the constants are derived in the space they are applied in:

- **Stereo combines in power.**
  Band powers, air flatness, localized peaks, the coverage floor and the tail ratio take USB-out 1/2 separately (`leveller::DoctorSignal`) and average their PSDs and RMS², not their waveforms, so a decorrelated stereo reverb tail keeps its full power and chorus or doublers do not comb-filter into notches the player never hears.
  The onset detector reads the per-sample average (`DoctorSignal::mix`).
  The reported `integrated_lufs` is the 2-channel BS.1770 sum every other output-side measurement uses (`leveller::doctor_capture_with_loudness`).
- **The body excludes the tail and the pad.**
  The body PSD spans `[signal_start, body_end)` (`doctor::body_psd_span`), and the stimulus PSD spans the stimulus without its pad (`leveller::stim_body_psd`): same length, same Welch grid, so a wire chain's transfer is flat to < 0.05 dB and a wet preset's decay does not mix into its band powers.
  The tail ratio's body RMS starts at the signal too.
- **Band edges are half-open** (`[lo, hi)`), so a 3 or 6 kHz edge that lands exactly on a bin counts once.

**Choreography.**
Consecutive scene sounds of the same preset skip the per-sound preset reload (`doctor_skip_load`, only when the previous sound wrote nothing and succeeded; base and footswitch sounds always reload), and the capture connections use the lean handshake (`Session::connect_lean`).
Every capture is checked before it is diagnosed (`commands/doctor.rs`'s `capture_suspect`): a floor read (the momentary floor guard), an un-aligned onset (the path a silent inject takes), or a capture that ends before the body does (no tail window, which `tail_energy_ratio` would read as its −80 dB "bone dry" sentinel) retries once after a quiet gap with a fresh load, then reports the sound as an error, never a diagnosis.
The coverage gate's pre-onset floor averages 2048-point Welch segments (`psd::welch_psd_floor`) and compares by mean band density, because one 8192-point segment scattered the narrow low bands' floor by 1.5–2.5 dB run to run [HW].
The stimulus PSD is computed once per stimulus, not per capture.
A single capture can occasionally misread; repeated runs are the arbiter.

## Prescriptions

`Rx` derivation is graph-aware (`graph_facts`): fixes prefer an existing carrier block over inserting one, inserts are gated by the `blockcaps` limits, comp-aware rules avoid stacking compressors, and parallel-split placements the wire cannot express are skipped.
The EQ move (`eq_move`) has three tiers: a drivable EQ-10 stereo already in the chain gets a value-aware one-click; a different EQ already present (`OTHER_EQ_IDS`) gets an advisory to use it; with no EQ, an EQ-10 is inserted right after the cab (`after_cab_anchor`).
The insert anchor is the neighbour's node id, looked up among the group's current node ids, because an anchor that is not one of them aborts the device server ([device/structural-edits.md#insert-abort](device/structural-edits.md#insert-abort)).
Parameter one-clicks are value-aware wherever the current value rides the graph allowlist: a write that would move a known value the wrong way is dropped, and a blind write on an unknown value says so in its `Rx::detail`.
The muddy and harsh EQ cuts grow with the finding's margin past its gate (`sized_cut_db`: the shipped 3 dB / 2 dB plus the margin, capped at 6 dB), never below the shipped size.
The app shows each fix's `Rx::title` only, as a bullet under "What to change", in the backend's order; the `ops` stay on the wire (the shared-block line reads which blocks they touch) but the app never applies them.
`severity.ts` gives each finding its tone and the one worst-first order (`findingTone`, `sortedDiags`).

**Leveling-damage advisories** (`doctor::leveling_damage_hints`) are a backup-scan-only, zero-capture scan of a preset's footswitch `param` assignments for two shapes the current `param_class` gate could never have produced: a wet-mix param assign-leveled to ~0 (an effect silenced whenever the switch engages) and an Other-class param that was swept (never a level control).
They surface per preset as an advisory-only "Footswitches" row (a sibling of the "Scenes" row, caution tone, "2 footswitches worth checking") with factual copy only ("Turns the effect almost silent when you press it."), never "damage", since a footswitch can legitimately sweep any parameter through Pro Control.
The row sorts in with the findings, so a preset whose only finding is this advisory still shows.

## Cut-through and reference match

- **Cut-through** (`doctor::cut_through` → `DoctorSoundResult.cutThrough`) is the presence contrast: high-mids + highs over lows..mids band power, in dB (Air and the Bass VI Sub band excluded).
  It carries a percentile against the pinned 248-value factory-bank distribution (`FACTORY_CONTRAST_DB`) and an advisory below `CUT_THROUGH_ADVISORY_DB` = −6.2 dB.
  An opened sound shows it as "Cutting through the mix" ("More presence than 62% of the factory presets."), hidden when there is no factory comparison.
  It is guitar-anchored: bass families report the contrast only, since there is no bass factory distribution.
  It is an estimate card, not a diagnosis: no rule, no `Rx` ops.
- **Reference match** (`matchModel.ts` + `resultsModel.ts::matchAdvice`) is fully client-side.
  "Use as reference" works on any sound of the run; other same-layout sounds get "To sound like <reference>" with EQ-10 moves from the `balanceDb` deltas already on the wire.
  The moves center on the median body-band delta, so Air and Bass VI's Sub cannot shift every other move through the all-band mean; they use the log-nearest band, combine when bands collide, clamp to ±6 dB and drop moves under 1.5 dB.
  They read as one step: on the chain's own 10-band EQ, on a new one after the cab, or (with no CPU room for one) on an EQ already there.
  When any delta exceeds EQ reach it adds "EQ only gets part of the way. A different cab or amp gets closer."
  It costs no new captures, commands or wire fields.

## Validation arms

- `probe --doctor-inject <slot> <gains_csv|none>`: a single EQ-10 defect injection with before / after verdicts; it also verifies the EQ-10 band control ids on the unit (a wrong id shows as the defect not appearing).
- `probe --doctor-defects <slot> [--out r.json]`: the versioned defect-recipe sweep (control / muddy / lost / washed / resonant_wah / resonant_peq / boxy_peq); each recipe's ops inject live, with a HIT / MISS / VIOLATION table against `must_fire` / `must_not_fire`.
  The recipe table in `doctor_defects.rs` is the fixture set.
- `probe --doctor-window-ab`: the window against the pinned 6 s oracle.
- `probe --doctor-calib`: threshold-space sweeps ([doctor-thresholds.md#calibrated-di-captures](doctor-thresholds.md#calibrated-di-captures)).

Captures are Float32 WAV (fmt tag 65534, `WAVE_FORMAT_EXTENSIBLE`): Python's `wave` module rejects them (`unknown extended format`), so parse the fmt and data chunks by hand or use a float-aware reader; Rust `hound` round-trips them.

## Words on screen

`doctor.rs` writes every Doctor sentence the app shows: each finding's `label` and `explain`, and each fix's `Rx::title`.
They follow the app's voice: plain guitarist's words, one idea per sentence, no dashes, semicolons or hedging ("may", "a touch"), no Hz figures or knob values.
They use the band names the band meter shows (`LABELS_6`: Lows, Low mids, Mids, High mids, Highs, Air), which are display only; a band's identity is its index (`Family::semantic_bands`).
A finding's name never carries a frequency ("Boxy", "Ringing"); where a fix has to say where, it names the band ("cut a narrow notch in the high mids").

Fixes come simplest first: a knob on a block the preset already has (the amp's, a cab's, an EQ's, a reverb's, a compressor's), then a block to add, and every fix after the first starts with "Or".
`knob_first` puts the amp-knob step ahead of a cab-filter fix that has to insert a block; muddy, harsh, lost and buried always lead with their knob step.
The exact settings behind each step (bands in Hz, dB, Q, the cab filter's frequency, whether a write is relative or absolute) ride `Rx::detail`, and the reading behind each finding rides `Diag::detail`; neither is for the screen, and `doctor_check` logs both (`log_findings`).
`Diag::direction` (`strong` | `weak`) says whether the finding's bands are too strong or too weak, for the band meter's words.
The voice tests in `doctor.rs` pin all of this: `every_fix_reads_simplest_first_in_the_app_voice`, `every_finding_names_itself_without_figures_and_says_which_way_its_bands_lean`, `scene_advice_reads_in_the_app_voice`.

## Scene consistency

Separate from the tonal rules, a scene whose loudness jumps ≥ `scene_delta_db` = 3 dB from base is flagged as an advisory-only `SceneConsistency`.
The Doctor has no in-app scene trim (the wire cannot set a scene's loudness relative to base, and Level-tab leveling targets an absolute LUFS), so every branch advises a step on the unit: a louder scene turns its amp's Level down until the jump is within the gate (or raises the Middle to stand out instead), a louder footswitch sound turns down the block it turns on, and a quieter one is turned up if it is not on purpose.
The app adds the Level-tab step itself, except when the louder sound is the first scene (wire scene 0): the opened row says the reading is not certain and adds no leveling step, although a USB `loadScene(0)` reaches the same state as selecting scene 0 on the unit ([device/scenes.md#panel-and-footswitch-scene-changes](device/scenes.md#panel-and-footswitch-scene-changes)).
`DoctorOp` carries no `SceneTrim` variant.

The steps name each sound as the screen does: `DoctorInput.sound_name` ("Lead", "footswitch 3"), which the frontend sends beside the wire `label`.
The label ("Tweed Warm · Lead") also names the preset, so it stays in the rows and `worst_name`, where the frontend finds the sound by it, and never in a sentence.

Only measured sounds enter the consistency check.
A Stop returns the sounds the run never reached with `status: "unrun"` and `integratedLufs: null`, never a placeholder reading, and the title never says "Nothing to change in these N sounds" (or "in this sound") while one is unrun or errored.

## Playback level

Every sound is diagnosed at all three playback levels at once, and each finding is tagged with the quietest level it fires at; there is no picker.
Equal-loudness contours flatten as SPL rises, so low-frequency (boomy, muddy) and mildly-HF (fizzy) content is perceptually hotter at stage volume; a preset can be clean quiet and boomy loud, and the Doctor shows that instead of hiding it behind a mode toggle.

The capture is level-independent (the offset shifts the comparison threshold, not the measured deviation), so this costs one ~5 s capture per sound and three pure passes over one `RuleMetrics` (`doctor::diagnose_levels` → `apply_thresholds` ×3), merged by diagnosis key.
The offsets (`doctor::playback_offsets`) are monotonic in loudness (louder ⇒ tighter or equal ⇒ never fewer firings, asserted by `playback_offsets_are_monotonic`), so a finding's firing set is always a louder-suffix and one ordinal describes it: `LeveledDiag.from_level` ∈ {quiet, rehearsal, stage}.
The UI shows this only inside an opened sound, in each finding's heading (`FindingBlock.tsx`): "Heard at every volume" / "Heard at rehearsal volume and up" / "Heard only at stage volume", beside three venue icons lit in the finding's tone where it fires.
The indicator is a read-only render of `from_level`, decoupled from the Settings `playback_level` store: the Doctor diagnoses all three levels whatever room level is chosen for leveling.
The offsets are additive at comparison time and never mutate the pinned `Thresholds` consts.
The marketing showcase's curated profiles sit far from every threshold, so they tag `quiet` at every level; the pinned `showcase_profile_diagnoses` test uses the offset-free `diagnose()`.
The offsets and their derivation are in [doctor-thresholds.md#playback-level-offsets](doctor-thresholds.md#playback-level-offsets).
