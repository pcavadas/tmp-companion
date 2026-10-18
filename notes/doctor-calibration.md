# Doctor threshold calibration

The procedure that derives the shipped guitar target and gates is in
`notes/doctor.md` ("Re-deriving on a new stimulus"). This file carries the
derivations and evidence behind specific gates.

## Capture-stimulus (calibrated DI) diagnosis

The Doctor diagnoses a calibrated profile's DI capture against the SAME
per-family threshold table a synthetic stimulus uses — `Family::thresholds()`
does not branch on `StimulusKind` (only the fizzy flatness gate does), and
diagnosis is per-sound and deterministic. Subtracting the stimulus's own band
shape is exact on a clean chain and over-corrects a driven one, which passes
only about half of a DI's Mids skew and so reads a spurious `lost` (HW).
Capture-space thresholds are therefore PROVISIONAL (open:
notes/open-questions.md). An attended sweep derives them:
`probe --doctor-calib <slots_csv> --stim <capture.wav> --family <fam>
[--labels labels.json] --out report.json` — deterministic JSON + markdown
(clean-population stats, labeled positives, midpoint/p95+margin proposals,
separation margins, pre-onset noise floor, stimulus band coverage). A
DI-specific recalibration would edit `GUITAR`/`BASS`/`BASS_VI` in `doctor.rs`.

Band gating is the captured output's own SNR (`notes/doctor.md`); the
input-side `doctor::band_coverage` feeds only the calibration readout
(`calibrate_profile`). `balance()` feeds only the display `balanceDb` and the
cut-through contrast, never a verdict.

Captures are **Float32 WAV** (fmt tag 65534 / `WAVE_FORMAT_EXTENSIBLE`) — Python's
`wave` module rejects them (`unknown extended format`); parse the fmt/data chunks
manually (or use a float-aware reader). Rust `hound` round-trips them.

## Playback level (equal-loudness) threshold offsets — ISO 226:2023

`doctor::playback_offsets` shifts the boomy, muddy and fizzy gates by playback
level. `doctor_check` diagnoses every sound at **all three** levels
(`doctor::diagnose_levels`) and tags each finding with the quietest level it
fires at.

**Derivation.** Source: ISO 226:2023, _Acoustics — Normal equal-loudness-level
contours_, eq. (1) with the Table 1 parameters α_f, L_U and T_f (α_r = 0.300,
T_r = 2.4 dB). Nominal loudness levels: **Quiet 60, Rehearsal 70, Stage 80
phon**. 80 phon is the standard's ceiling for the 5–12.5 kHz points the Air
band needs (20 Hz–4 kHz is valid to 90 phon), so Stage is a floor for a real
stage, which runs louder; 60 phon is also the level BS.1770's loudness tests
were run at (see `notes/leveling.md`, "Playback-level (Fletcher–Munson)
compensation").

For each ⅓-octave point `f` inside an analysis band, the gain the ear adds going
from 70 to 80 phon, relative to 1 kHz, is `(L_f(f,70) − 70) − (L_f(f,80) − 80)`.
The band value is the mean over its points:

| Band              | Lows  | Low-mids | Mids  | High-mids | Highs | Air   |
| ----------------- | ----- | -------- | ----- | --------- | ----- | ----- |
| 70 → 80 phon (dB) | +3.39 | +1.77    | +0.39 | −0.23     | −0.39 | +0.18 |
| tilt-split local  | +0.71 | 0.00     | −0.35 | 0.00      | +0.71 | +1.95 |
| median-centered   | +3.00 | +1.38    | 0.00  | −0.62     | −0.78 | −0.21 |

That vector is run through the rule metrics themselves (`tilt_split`, whose
Theil–Sen slope takes −0.67 dB/oct of it, and `centered_deviations`), and each
gate moves by minus its metric's shift, rounded to 0.1 dB. The fizzy metric
(Air − Highs) moves +0.57. 70 → 60 phon mirrors 70 → 80 within 0.03 dB, so
Quiet is Stage negated:

| Gate                   | Stage       | Quiet       |
| ---------------------- | ----------- | ----------- |
| boomy local / centered | −0.7 / −3.0 | +0.7 / +3.0 |
| muddy local / centered | 0.0 / −1.4  | 0.0 / +1.4  |
| fizzy                  | −0.6        | +0.6        |
| harsh                  | 0           | 0           |

boomy moves at least as far as muddy in both spaces, as the contours require.
**harsh is decided at 0:** its local metric does not move, and its centered
metric moves the other way (High-mids reads 0.62 dB _less_ prominent against
the body at 80 phon), so a derived shift would relax harsh as playback gets
louder. That breaks the louder-suffix model `diagnose_levels` reports with, for
0.6 dB in one consensus space. Harshness growing with level is the whole tone
getting louder; the band balance the Doctor judges barely moves (−0.23 dB).

Limits: the shift adds exactly to a sound's own centered deviation and to first
order to its Theil–Sen local; the band mean ignores the sound's spectrum inside
the band; and the dark/bright tilt rules stay SPL-blind although the same
contour change tilts a tone −0.67 dB/oct darker at Stage. The **Rehearsal =
offset 0** anchor is the ASSUMED monitoring level the `Thresholds` were
calibrated at: no calibration sweep recorded monitor SPL. The next attended
sweep should **record the monitor SPL** and confirm it; if it is not near 70
phon, re-anchor. Offsets are additive at comparison time and never mutate the
pinned consts, so this is a separate tuning axis from a capture-space sweep.

## Localized-rule evidence (HW, former stimulus)

The `resonant`/`boxy` gates were placed on the band-passed-noise stimulus the
pitched plucks replaced; the facts below are about the device and the
estimator, not that stimulus.

**Schema** (`ACD_FiveBandParamEQ`, from the user bank's own presets + HW write-verify):
per band N=1..5, `filterNfrequency` (Hz), `filterNgaindb` (dB), `filterNq`, `filterNtype`
(2 = peak), `filterNbypass` — all live `changeParameter` controlIds (1 kHz/+12 dB/Q8 in →
1008 Hz/q 7.5 measured). Band 1 defaults to a HIGH-PASS (setting its frequency collapsed
the lows −16 dB and fired bright/thin); bands 2–4 default to active peaks.

**Matrix** (`probe --doctor-inject … --block ACD_FiveBandParamEQ`, drives→'65 Deluxe+CabIR
chain): gains +6/+9/+12 dB × Q 2/4/8/12 at 700/2000/5600 Hz + a 400/1200 Hz
boxy pass + a stacked 2×12 dB Q14 flagrant-ring probe. Findings:

- **Measured q is site-dominated, not injection-recoverable** (Q2 and Q12 at the 700 Hz
  site both read q ≈ 5–6); q's only honest job is excluding isolated comb needles
  (q 85–455). The estimator also INFLATES q for strong rings (injected Q14 → q 25.4),
  hence the ceiling of 40.
- **Heights add in dB on structured sites** (700 Hz: 8.0 native + 8.3 injected → 16.3
  measured) and **saturate on clean sites** near 20·log10(Q/2) (Q8 +12 dB → 8.3).
- **The factory population below 4 kHz tops out at h = 12.6** (a synth preset's own
  filter), and most factory peaks live ABOVE 4 kHz (comb forest, ungateable) → gates:
  freq ≤ 4 kHz, resonant h ≥ 13.5, boxy (300–500 Hz, no factory peaks in range)
  h ≥ 7.5, q ∈ [2, 40], + band corroboration.
- **The cocked wah is a band-space phenomenon by physics** (peak-space h ≈ 6.7, mid local
  +15.7 dB) — pinned as the `resonant_wah` must-NOT-fire-resonant defects recipe.

The defects suite hit 7/7 on that stimulus (`resonant_peq` stacked 2×12 dB Q14 @
2.6 kHz → resonant; `boxy_peq` stacked 2×12 dB Q8 @ 420 Hz → boxy; wah → thin
only); whether it, factory silence and the parametric-cut Rx hold on the shipped
stimulus and the current Q estimator is open (notes/open-questions.md).
