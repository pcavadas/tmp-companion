# Doctor thresholds

Read this when changing the Doctor's metric, a threshold, its derivation or the playback-level offsets.

The flow, rules and prescriptions are in [doctor.md](doctor.md).
Facts are for firmware 1.8.58 unless a line says otherwise.

## Metric

Each capture's band powers (one shared post-onset body Welch PSD, `doctor::body_psd`) become `deviation[i] = band_db[i] − TARGET[i]` against an authored per-family target curve.
`GUITAR_TARGET` is the median shape of the 248-sound guitar factory bank through the pitched-pluck humbucker stimulus; the bass and Bass VI targets (`BASS_TARGET`, `BASS_VI_TARGET`) are provisional few-anchor curves from user slots 21 and 8–9, not derived from a factory sweep.
Band powers are width-integrated (a per-Hz-density target misread every factory preset as bright and lost).
A Theil–Sen (median-slope) fit splits the deviations into a broadband tilt (the dark / bright / thin lean) and per-band locals.
Every band rule (muddy / boomy / harsh / lost / thin / buried) fires only on a two-space consensus: the tilt-split local and the median-centered deviation (`centered_deviations`) must each clear their own gate, and severity is the smaller margin.
Why two spaces: defect injection (`probe --doctor-inject`) showed the tilt split alone misattributes a skirted single-band defect (+12 dB at 250 Hz read as a false "thin"), while the centered space alone is contaminated by healthy tilt at the endpoint bands [HW].
The centered space centers on the median of the covered body bands only, so a band the capture never excited cannot move a covered band's verdict; with fewer than 3 covered body bands (`MIN_CENTERING_BANDS`) it is unknown, and every consensus rule (and the localized rules, which corroborate in it) abstains.

Severity stays in the rule's own unit (dB, dB/oct) for display.
Hedging and ranking read `Diag::z` = severity ÷ the rule's run-to-run σ (`doctor::rule_sigma`: band rules and fizzy 0.4 dB, tilt 0.05 dB/oct, peak height 0.3 dB, tail ratio 0.3 dB), and the UI calls a finding "Possible" below 2.5σ (`severity.ts::POSSIBLE_MAX_Z`; a band rule's 1.0 dB).
The metric is deterministic per sound: a verdict depends only on that sound's own measurements, never on which other sounds ran in the same check.

## Thresholds

The thresholds are constants in `doctor.rs`, one table per family: Guitar, Bass and BassVi (a 7-band layout with a (30, 60) Hz Sub band that is measured and displayed only; no rule keys on it).
`StimulusKind` does not select a table; a capture stimulus differs only through the fizzy flatness gate.
Recalibration edits the values in `doctor.rs` and nowhere else.

A rule whose primary band was never excited is skipped.
Coverage is judged on the captured output's own SNR (`output_coverage_with_body`: band power against the pre-onset noise floor plus `OUTPUT_SNR_MARGIN_DB`), not on the input stimulus, so amp-created HF (clipping harmonics, fizz) stays diagnosable on a dark DI input.
The input-side `doctor::band_coverage` feeds only the calibration readout (`calibrate_profile`), and `balance()` feeds only the display `balanceDb` and the cut-through contrast, never a verdict.
`DoctorSoundResult.skippedBandCount` counts the bands the SNR gate dropped (0 when no coverage was computed, for an errored or showcase sound); when it is non-zero the opened sound says "Some bands were too quiet to check." under its last finding.

## Re-deriving on a new stimulus

The guitar constants (`GUITAR_TARGET`, the `GUITAR` gates, `GUITAR_STIM_REF`, `FACTORY_CONTRAST_DB`, `CUT_THROUGH_ADVISORY_DB`) come from a factory sweep through the shipped pitched-pluck humbucker.
To redo it after a stimulus change:

1. `probe --doctor-calib-factory 0..254 --stim resources/samples/guitar-humbucker.wav --family guitar --out <report.json>` (~37 min, through `leveller::analyze_capture`).
   Keep the previous sweep: it is the baseline.
2. `probe --doctor-derive <report.json> --baseline <previous report.json>` prints the target (the bank's median shape), the contrast population and the gates.
   Each gate is rank-transferred (`benchmark::transfer_gate`): it keeps the percentile the shipped gate held in the baseline population, so every rule's false-alarm rate on healthy presets carries over.
   Paste `GUITAR_STIM_REF` from `print_guitar_stim_ref`.
3. `probe --doctor-loo <report.json>` gives the false-positive rate per rule against the baseline's ([benchmarks.md](benchmarks.md)); then run the defects suite (all 7 recipes must hit) and the empty-slot capture (no finding).
4. `probe --doctor-replay <capture.wav,…> --stim <wav>` re-runs the analysis on raw captures offline, so a detector change can be checked without the unit.

The rank transfer keeps the deliberately sensitive in-sample end: about half of the healthy guitar factory bank gets at least one finding [HW].
A whole-bank max-plus-margin derivation flags about a fifth, mostly `washed`, but widened `lost` by 21 dB and silenced the defect presets [HW].
The `washed` gate is −8.5 dB because the pitched stimulus lifts every tail ratio.
The `muddy` and `boomy` gates the Stage playback level tightens are floored at 0 dB, so a band sitting on the target never reads as excess.

## Localized-rule gates

The `resonant` and `boxy` rules are in [doctor.md#localized-rules](doctor.md#localized-rules).
The gates come from a defect-injection matrix (`probe --doctor-inject … --block ACD_FiveBandParamEQ`, drives into a '65 Deluxe + CabIR chain) run on a band-passed-noise stimulus; the facts below are about the device and the estimator, not that stimulus:

- **`ACD_FiveBandParamEQ` schema.**
  Per band N = 1..5: `filterNfrequency` (Hz), `filterNgaindb` (dB), `filterNq`, `filterNtype` (2 = peak) and `filterNbypass`, all live `changeParameter` control ids (1 kHz / +12 dB / Q8 in → 1008 Hz / q 7.5 measured) [HW].
  Band 1 defaults to a high-pass (setting its frequency collapsed the lows by 16 dB); bands 2–4 default to active peaks [HW].
- **Measured q is site-dominated, not injection-recoverable**: Q2 and Q12 at the 700 Hz site both read q ≈ 5–6 [HW].
  q's only honest job is excluding isolated comb needles (q 85–455), and the estimator inflates q for strong rings (an injected Q14 reads q 25.4), hence the ceiling of 40.
- **Heights add in dB on structured sites** (700 Hz: 8.0 native + 8.3 injected → 16.3 measured) and saturate near 20·log10(Q/2) on clean sites (Q8 at +12 dB → 8.3) [HW].
- **The factory population below 4 kHz tops out at h = 12.6** (a synth preset's own filter), and most factory peaks sit above 4 kHz in the ungateable comb forest [HW]; hence the gates: frequency ≤ 4 kHz, resonant h ≥ 13.5, boxy (300–500 Hz, no factory peaks in range) h ≥ 7.5, q in [2, 40], plus band corroboration.

## Onset alignment

`leveller::doctor_onset` places the body/tail split.
It tries a floor-relative energy step first, falls back to the envelope correlator (`audio::estimate_onset`: correlation ≥ 0.15 at a lag ≤ 120 ms) only when the step does not fire in band, and to the un-aligned split when neither does.

- **The correlator cannot place a wash onset.**
  On a 65%-wet `ACD_TMLargePlate` its correlation curve is flat (0.27–0.37 at every lag from 0 to 250 ms, no peak), so its confidence gate either rejects the capture or accepts an arbitrary lag, landing on either side of the 120 ms gate at random and flipping `washed` (`probe --doctor-fs`) [HW].
- **The energy step is deterministic** because the Doctor stimulus always carries the played silent pad ahead of the body: the first 2 ms hop that steps up ~60 dB from the pre-signal floor lands at 230–232 ms on every wet and dry chain observed, a true inject latency of ~30 ms [HW].
- **The split location matters on a wash.**
  A sustained reverb tail decays ~25 dB/s right after the body, so `tail_ratio_db` moves ~0.8 dB per 32 ms of split position on the wash fixture (0 ms un-aligned → −8.85, 32 ms true → −9.6, 48 ms → −10.1, 120 ms → −11.5) [HW].
  A wrongly confident correlator lag past ~48 ms is enough to flip `washed` on a marginal wash preset.
- **The tail length is pinned.**
  `reamp_capture` keeps whatever frames arrive in `play + tail` ms, and the tail length moves the ratio ~0.3 dB per 100 ms [HW], so the tail window is pinned to the recipe's tail (`DOCTOR_TAIL_MS`) after the body end.
- **The first capture after other device activity reads ~0.3 dB off**, a device-side cold-capture offset; warm runs spread σ ≈ 0.05 dB [HW].
  The oracle wash fixture sits at mix 0.9 with decay, `earlylatebalance` and `higheq` at 1.0, reading −6.7 to −6.8 dB on the pitched-pluck stimulus, a 1.7 dB margin over the gate [HW].
  Those three are the levers, because the body gets wetter with the tail as mix rises; at decay 1.0 alone the tail reads −8.3 to −8.4 dB, and the damping and diffusion controls only shorten it [HW].

## Calibrated DI captures

The stimulus is profile-aware (`resolve_stimulus_with_capture`): a calibrated profile's DI capture is injected verbatim and diagnosed against the same per-family threshold table as a synthetic stimulus; `Family::thresholds()` does not branch on `StimulusKind`.
A real DI shifts band balance systematically (+8..12 dB Lows, −8..10 dB Highs) [HW].
On a factory sweep through one real DI take, driven chains read `lost` no more often than clean ones with the shipped constants: 13% of 62 driven against 11% of 186 clean guitar presets, with the same Mids balance [HW].
Band thresholds re-derived from that sweep keep every band rule at or under 0.4% held-out false alarms [HW].
No derivation mode re-derives `washed`, `resonant` or `boxy`, which fire on 24%, 2.7% and 1.2% of that sweep and need capture-space rules of their own [HW].
The app still diagnoses a DI capture against the synthetic-stimulus table.
An attended sweep derives them: `probe --doctor-calib <slots_csv> --stim <capture.wav> --family <fam> [--labels labels.json] --out report.json` writes deterministic JSON and markdown (clean-population stats, labeled positives, midpoint and p95+margin proposals, separation margins, the pre-onset noise floor, stimulus band coverage).
A DI-specific recalibration would edit `GUITAR` / `BASS` / `BASS_VI` in `doctor.rs`.

## Playback-level offsets

Which playback levels a finding is heard at is in [doctor.md#playback-level](doctor.md#playback-level).
The offsets come from ISO 226:2023, _Acoustics — Normal equal-loudness-level contours_, eq. (1) with the Table 1 parameters α_f, L_U and T_f (α_r = 0.300, T_r = 2.4 dB).
The nominal loudness levels are Quiet 60, Rehearsal 70 and Stage 80 phon.
80 phon is the standard's ceiling for the 5–12.5 kHz points the Air band needs (20 Hz–4 kHz is valid to 90 phon), so Stage is a floor for a real stage, which runs louder.
60 phon is also the level BS.1770's loudness tests ran at ([leveling-calibration.md#playback-level-compensation](leveling-calibration.md#playback-level-compensation)).

For each ⅓-octave point `f` inside an analysis band, the gain the ear adds going from 70 to 80 phon, relative to 1 kHz, is `(L_f(f,70) − 70) − (L_f(f,80) − 80)`; the band value is the mean over its points:

| Band | Lows | Low-mids | Mids | High-mids | Highs | Air |
|---|---|---|---|---|---|---|
| 70 → 80 phon (dB) | +3.39 | +1.77 | +0.39 | −0.23 | −0.39 | +0.18 |
| tilt-split local | +0.71 | 0.00 | −0.35 | 0.00 | +0.71 | +1.95 |
| median-centered | +3.00 | +1.38 | 0.00 | −0.62 | −0.78 | −0.21 |

That vector runs through the rule metrics themselves (`tilt_split`, whose Theil–Sen slope takes −0.67 dB/oct of it, and `centered_deviations`), and each gate moves by minus its metric's shift, rounded to 0.1 dB.
The fizzy metric (Air − Highs) moves +0.57.
70 → 60 phon mirrors 70 → 80 within 0.03 dB, so Quiet is Stage negated:

| Gate | Stage | Quiet |
|---|---|---|
| boomy local / centered | −0.7 / −3.0 | +0.7 / +3.0 |
| muddy local / centered | 0.0 / −1.4 | 0.0 / +1.4 |
| fizzy | −0.6 | +0.6 |
| harsh | 0 | 0 |

Boomy moves at least as far as muddy in both spaces, as the contours require.
Harsh stays at 0: its local metric does not move, and its centered metric moves the other way (High-mids reads 0.62 dB less prominent against the body at 80 phon), so a derived shift would relax harsh as playback gets louder and break the louder-suffix model for 0.6 dB in one consensus space.
Harshness growing with level is the whole tone getting louder; the band balance the Doctor judges barely moves (−0.23 dB).

Limits: the shift adds exactly to a sound's own centered deviation and, to first order, to its Theil–Sen local; the band mean ignores the sound's spectrum inside the band; and the dark / bright tilt rules stay SPL-blind although the same contour change tilts a tone −0.67 dB/oct darker at Stage.
Rehearsal (offset 0) is the assumed monitoring level the `Thresholds` were calibrated at; no calibration sweep recorded monitor SPL, so a sweep that records it and finds it far from 70 phon re-anchors the offsets.
Offsets are a tuning axis separate from a [capture-space sweep](#calibrated-di-captures).
