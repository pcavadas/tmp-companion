# Doctor threshold calibration — 2026-07-03

> **2026-07-16 addendum:** three things changed since this document's sweeps —
> the numbers below are the historical record of the ORIGINAL recipe/metric.
>
> 1. **Window**: 3 s stimulus slice + 200 ms pad + graph-aware 1.5 s/0.3 s tail
>    (HW-A/B'd vs the 6 s + 2.5 s oracle, `probe --doctor-window-ab`, 0 flips).
> 2. **Onset**: corr floor 0.15 + a 120 ms lag plausibility ceiling + the pad's
>    silence→signal edge FIXED the "onset detection fails even on the synthetic
>    stimulus" weakness noted below — onsets now resolve confidently on all
>    sweep presets, including wet ones (dry tails measure −21..−24 dB truer).
> 3. **Metric**: deviation vs the AUTHORED factory-median target + Theil–Sen
>    tilt split + the two-space consensus (see `notes/doctor.md`); gates were
>    re-derived 2026-07-16 from the ±12 dB HW defect-injection matrix
>    (`probe --doctor-inject`) + the 25-slot factory sanity band. The
>    cohort-median machinery this document tuned against is deleted.

Method: `probe --doctor <slots> <topology>` sweeps on the real unit (fw-current,
read-only: loads + captures with the 2.5 s Doctor tail, never a save), then
tuning `doctor::Thresholds` until the fired diagnoses matched the presets'
known characters. 14 guitar sounds (`0–7, 11–16`, guitar-humbucker stimulus,
cohort-median mode) + the 2 Bass VI presets (`8, 9`, bass-singlecoil,
absolute-fallback mode, cohort < 4).

## What validated as-shipped

- **washed** (`wash_tail_db = −13`): caught exactly the wash-heavy presets —
  Shoegaze (tail −0.3 dB under the note), Reverse Delay (−6.9), a synth pad
  (−2.7) — while every dry preset sat at −17…−25 dB. The 2.5 s
  `DOCTOR_TAIL_MS` window is what makes this measurable; the leveling 0.8 s
  tail truncates decay. The body/tail split is now **onset-aligned**
  (`audio::estimate_onset`); a 2026-07-11 exemplar re-sweep confirmed −13
  holds under it (tails moved ≤ ±0.4 dB — latency is negligible on the
  reference rig, the alignment is defensive hardening).
- **muddy / boomy** (4.5 / 5.0 dB over cohort): fired together on the one
  genuinely dark preset (+14.6 dB lows, +10.6 dB low-mids vs library median),
  silent elsewhere. On bass, +12 dB lows did NOT flag boomy (absolute dev vs
  neighbour 5.9 < 6.0) — correct: lows are a bass's job.
- **harsh** (5.0): fired on the two known-peaky presets (+8.9 / +7.0 dB
  high-mid spikes) plus the Bass VI's 1–3 kHz clank (+8.7).
- **lost** (4.5 / 5.0): fired on two scooped guitar presets — and on BOTH
  Bass VI presets via the absolute path (mids −5.8 / −7.4 dB), matching the
  player's own "my Bass VI gets lost in the mix" complaint. Ground truth
  doesn't get better than that.

## What was re-derived: fizzy

The original rule (Air excess vs cohort median) flagged 5 of 14 presets,
including the player's bread-and-butter clean sounds. Root cause: the Air band
is **bimodal** across a library — cab'd presets roll off 25–44 dB above 6 kHz,
open/IR-light ones only 10–20 — so the cohort median is pathologically low and
everything without steep rolloff reads +12…+18 dB "excess".

Fizz is a property of the sound itself: HF hash extending past 6 kHz, i.e. Air
FAILING to roll off below the presence band. The rule is now own-spectrum:
`balance[Air] − balance[Highs] > fizzy_db` with `fizzy_db = −9.0` ("air less
than 9 dB under the presence band"). Under this rule the library's open
presets sit at −10…−21 (silent) and cab'd ones at −25…−44; nothing in the
current library fires, which matches ears — none of it is actually fizzy
through a cab sim.

## Notes for future re-calibration

- Stimulus spread (`spread_lu` 0.12–0.8 across the library) barely
  discriminates — the shaped-noise stimulus is dense. The `buried` rule
  therefore keys on the lows deficit + a drive block in the graph, not spread.
- Probe's field-8 slot read returns truncated JSON, so `rx` arrays are empty
  in sweep output (prescriptions need the graph). The app path feeds full
  backup JSON; engine unit tests cover rx generation.
- Re-run: `cargo run --bin probe -- --doctor 0,1,2,3,4,5,6,7,11,12,13,14,15,16
guitar-humbucker` and `--doctor 8,9 bass-singlecoil`.
- `spiky` keys on `spread_lu > 4.0` (provisional, both instruments). The
  0.12–0.8 LU library range above was measured under the 0.8 s **leveling**
  capture; the Doctor capture's 2.5 s tail could in principle inflate spread on
  wet presets, so a fresh doctor-capture baseline was swept (2026-07-09, the
  re-run commands above): **0.12–0.81 LU across all 16 sounds** — the tail
  inflation did not materialize (the wettest preset, slot 3 at tail −2.7 dB,
  topped out at 0.81), `spiky` fired on zero library presets (by design: only
  envelope-heavy sounds — swells/tremolo/delay buildup — can cross 4 LU on this
  dense stimulus), and no washed co-firing was observed, so the contingent
  `tail_ratio_db` gate was not needed. The probe sweep's per-sound JSON
  `profile` carries `spreadLu`, directly usable to re-derive the value. The
  sweep's graph facts come from the truncated field-8 JSON, so `has_drive`
  there is unreliable — sanity-check any firing preset's drive blocks against
  the real graph (Pro Control / the backup scan) before drawing threshold
  conclusions.
- **Update (real-DI factory sweep, 2026-07):** on real Telecaster/ES-335 DI
  captures the max observed `spread_lu` was ~1.88 vs the 4.0 gate, so `spiky` is
  effectively **dead** (unreachable), not merely quiet — a retune should target the
  real spread distribution, not chase the 4.0 gate. `washed`/tail was separately
  fragile AT THE TIME of this sweep: its onset detection failed even on the
  synthetic stimulus (a structural weakness, not DI-specific) — HISTORICAL; the
  pad edge + recalibrated onset logic (point 2 at the top of this file) fixed
  exactly this, and onsets now resolve confidently on all sweep presets.

## Capture-stimulus recalibration (pending the attended sweep)

The Doctor diagnoses a calibrated profile's DI capture against its own
threshold table: `StimulusKind::Capture` selects the `*_CAPTURE` tables
(currently byte-identical copies of the synthetic ones — PROVISIONAL).
Diagnosis is per-sound and deterministic (the deviation-vs-authored-target
consensus metric never pools measurements across sounds), so a capture is only ever
compared against the capture-space table — the measured band-balance shift
between stimuli (+8…12 dB lows / −8…10 dB highs) would otherwise reproduce
false verdict flips if a capture were judged against the synthetic table.
Band-confidence
gating skips any band-keyed rule whose primary band the stimulus never
excited (≥30 dB under its loudest band), protecting sparse takes (e.g.
EBow-heavy) from verdicts in bands they never probed.

**RESOLVED (2026-07-16) — the `balance()` dead-band contamination is
structurally gone under the shipped metric.** The old bug: `balance()`
subtracts the mean of ALL 6 bands, and in capture space the floor-riding
Highs/Air dragged that mean, inflating the live bands ±3 dB preset-dependently
(it flipped a real `muddy` in a 16-preset factory sweep). The shipped rule
path has NO all-band mean anywhere: `deviations()` is raw `band_db − target`
(level absorbed by the Theil–Sen intercept, which fits COVERED bands only),
and `centered_deviations()` medians over the BODY bands (`lows..=highs`,
excluding Air and the Bass VI Sub). `balance()` survives only for the wire's
display `balanceDb` and the cut-through contrast (a ratio, where the mean
cancels) — neither feeds a verdict threshold.

The attended sweep derives the real capture thresholds:
`probe --doctor-calib <slots_csv> --stim <capture.wav> --family <fam>
[--labels labels.json] --out report.json` — deterministic JSON + markdown
(clean-population stats, labeled positives, midpoint/p95+margin proposals,
separation margins, pre-onset noise floor, stimulus band coverage). Replace
the `*_CAPTURE` consts in `doctor.rs` from that report; the pinned
`thresholds_for(Synthetic)` test guards the synthetic tables against drift.

Captures are **Float32 WAV** (fmt tag 65534 / `WAVE_FORMAT_EXTENSIBLE`) — Python's
`wave` module rejects them (`unknown extended format`); parse the fmt/data chunks
manually (or use a float-aware reader). Rust `hound` round-trips them.

## Playback level (Fletcher–Munson) threshold offsets — PROVISIONAL

`doctor::playback_offsets` shifts the boomy/muddy and fizzy thresholds by
playback level. `doctor_check` diagnoses every sound at **all three** levels
(`doctor::diagnose_levels`) and tags each finding with the quietest level it
fires at — it no longer reads a single level from the store. It is anchored at
**Rehearsal = offset 0**, the ASSUMED monitoring level the synthetic `Thresholds`
were calibrated at above (a working assumption, not measured — the 2026-07-03
sweep did not record monitor SPL). Stage tightens (boomy/muddy −2.0 dB, fizzy
−1.0 dB), Quiet relaxes (+2.0 / +1.0); values are coarse and PROVISIONAL.

The attended re-sweep should **record the monitor SPL** at capture time and
re-derive these offsets against measured equal-loudness behaviour (and confirm
which playback level the base `Thresholds` actually correspond to — if it is not
Rehearsal, re-anchor). Offsets are additive at comparison time and never mutate
the pinned consts, so this is a separate tuning axis from the `*_CAPTURE` sweep.

### Field data point — preset 001 base vs "Dist" scene (2026-07-13)

A player reported preset 001 (list index 0) reading all-clear in the Doctor yet
sounding **boomy at rehearsal volume**, on both the base and the "Dist" scene.
Read-only `probe --doctor 0,1,…,16,0:5 guitar-humbucker` (14 library bases +
preset 001 scene 5 = "Dist"), cohort-median mode:

- **Base (slot 0):** lows balance **+1.0 dB**, `dev(lows) ≈ 0.0` vs the ~+1.0 dB
  library median → **not boomy at any playback level** (not even Stage's 3.0 dB
  gate). The base is actually presence/highs-forward (Highs +13.3 dB). So the
  boom the player hears on the _base_ is **not in this preset's low end** — it
  supports his own "might be the other guitarist" hypothesis for the base.
- **"Dist" scene (slot 0 scene 5):** lows balance **+8.1 dB**, `dev(lows) = +7.0
dB` vs the library median → **boomy FIRES**. Under the provisional playback
  offsets the boomy gate is 7.0 (Quiet) / 5.0 (Rehearsal) / 3.0 (Stage), so the
  Dist scene crosses at Rehearsal-and-louder and sits right on the boundary at
  Quiet — matching the player's "boomy at rehearsal levels" report almost
  exactly. **His ear is confirmed for the Dist scene.**

Two live validations fell out of this: (1) the per-preset cohort dedupe (a run of
just preset 001's base + scenes would previously self-normalize — the Dist scene's
low end judged against the preset's own scenes, masking the boom → "Doctor says
fine"); (2) the playback-offset model — the Dist boom is exactly a
rehearsal-and-louder phenomenon, not a bedroom one. Left as an open data point:
the base reads flat here but was reported boomy by ear; if a future SPL-anchored
sweep still finds the base flat, the base-boom is environmental (room/other
guitarist), not a preset defect.

## 2026-07-17 — parametric-EQ ground-truth round (resonant/boxy ENABLED)

**Schema** (`ACD_FiveBandParamEQ`, from the user bank's own presets + HW write-verify):
per band N=1..5, `filterNfrequency` (Hz), `filterNgaindb` (dB), `filterNq`, `filterNtype`
(2 = peak), `filterNbypass` — all live `changeParameter` controlIds (1 kHz/+12 dB/Q8 in →
1008 Hz/q 7.5 measured). Band 1 defaults to a HIGH-PASS (setting its frequency collapsed
the lows −16 dB and fired bright/thin); bands 2–4 default to active peaks.

**Matrix** (`probe --doctor-inject … --block ACD_FiveBandParamEQ`, drives→'65 Deluxe+CabIR
chain, list index 16): gains +6/+9/+12 dB × Q 2/4/8/12 at 700/2000/5600 Hz + a 400/1200 Hz
boxy pass + a stacked 2×12 dB Q14 flagrant-ring probe. Findings:

- **Measured q is site-dominated, not injection-recoverable** (Q2 and Q12 at the 700 Hz
  site both read q ≈ 5–6); q's only honest job is excluding isolated comb needles
  (q 85–455). The estimator also INFLATES q for strong rings (injected Q14 → q 25.4), so
  the ceiling moved 16 → 40.
- **Heights add in dB on structured sites** (700 Hz: 8.0 native + 8.3 injected → 16.3
  measured) and **saturate on clean sites** near 20·log10(Q/2) (Q8 +12 dB → 8.3; Q9
  synthetic ceiling ≈ 13 at any drive).
- **The factory population below 4 kHz tops out at h = 12.6** (a synth preset's own
  filter), and 55/75 factory peaks live ABOVE 4 kHz (comb forest, ungateable) → gates:
  freq ≤ 4 kHz, resonant h ≥ 13.5, boxy (300–500 Hz, zero factory peaks in range)
  h ≥ 7.5, q ∈ [2, 40], + band corroboration unchanged.
- **The cocked wah is a band-space phenomenon by physics** (peak-space h ≈ 6.7, mid local
  +15.7 dB) — pinned as the `resonant_wah` must-NOT-fire-resonant defects recipe.

**Validation**: defects suite 7/7 HIT (control silent; muddy/lost/washed hit;
`resonant_peq` stacked 2×12 dB Q14 @ 2.6 kHz → resonant, measured h 24.2; `boxy_peq`
stacked 2×12 dB Q8 @ 420 Hz → boxy; wah → thin only). Factory silence holds by
construction (no in-range factory peak reaches either floor at any q). The probed
preset's stored bytes were sha-verified untouched across all ~20 injection runs.
