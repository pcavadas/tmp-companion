# Doctor threshold calibration — 2026-07-03

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

## Capture-stimulus recalibration (pending the attended sweep)

The Doctor now diagnoses a calibrated profile's DI capture in its own space:
`StimulusKind::Capture` selects the `*_CAPTURE` threshold tables (currently
byte-identical copies of the synthetic ones — PROVISIONAL) and cohorts are
keyed `(Family, StimulusKind)` so synthetic and capture sounds never pool
(the measured band-balance shift between stimuli — +8…12 dB lows / −8…10 dB
highs — would otherwise reproduce false verdict flips). Band-confidence
gating skips any band-keyed rule whose primary band the stimulus never
excited (≥30 dB under its loudest band), protecting sparse takes (e.g.
EBow-heavy) from verdicts in bands they never probed.

The attended sweep derives the real capture thresholds:
`probe --doctor-calib <slots_csv> --stim <capture.wav> --family <fam>
[--labels labels.json] --out report.json` — deterministic JSON + markdown
(clean-population stats, labeled positives, midpoint/p95+margin proposals,
separation margins, pre-onset noise floor, stimulus band coverage). Replace
the `*_CAPTURE` consts in `doctor.rs` from that report; the pinned
`thresholds_for(Synthetic)` test guards the synthetic tables against drift.
