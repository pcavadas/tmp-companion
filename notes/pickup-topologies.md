# Pickup topologies & aliases — what the templates encode and how to assign a pickup

Reference for `src-tauri/src/topologies.rs` (`TOPOLOGIES` + `ALIASES`). Read this
before adding an alias or a new topology. Adversarially reviewed against measured
pickup data (2026-07).

## What a template encodes

A topology is a synth recipe for the leveling stimulus (`gen_samples`), NOT a
pickup model. Three parameters matter for assignment:

- **`freq`** — the stimulus's spectral peak. For the **guitar** rows it
  approximates the pickup's _loaded_ electrical resonance (coil L + cable/pot C);
  for the **bass** rows it is a low-mid energy centroid shaping bass program
  material — real loaded bass-pickup resonance is ~2–3.5 kHz, nowhere near the
  rows' 550–800 Hz. Don't "correct" the bass rows: the WAVs are shipped and
  HW-validated.
- **`q`** — resonance width. Note the shipped values are template voicing, not
  physics (a real Strat coil has _higher_ loaded Q than a covered humbucker).
- **`peak`** — output as **amp-input drive** for uncalibrated profiles.

Hum-cancelling appears nowhere. Construction (single-coil vs humbucker) and
voicing are independent axes; **templates follow voicing**.

## Assignment methodology (why decimals don't matter)

Three measured facts from this repo's own probes set the error budget:

1. Synthetic-vs-real floor: a Tele DI vs the LUFS-matched synthetic measured
   **−5.4…+2.2 LU preset-dependent** error (`probe --stim-ab`). Every
   intra-family spectral distinction is second-order against this; Tier-2
   calibration is the real fix.
2. HW noise floor: ~**0.12 LU** run-to-run on non-stationary presets.
3. Amp compression: ~**1.65 LU per 6 dB** of input drive on a clean Twin
   (`probe --agc-test`), so a 0.05 `peak` delta (~0.9 dB) moves the result
   ~0.25 LU — at the noise floor. `peak` deltas between similar pickups are
   ignorable. (Calibration does NOT "absorb" them — it replaces the stimulus
   entirely; templates only ever serve uncalibrated profiles.)

So an alias only has to land the gross **brightness class** (bright-SC ~5.5 kHz
vs dark-HB ~3 kHz vs flat-active), judged by **measured inductance → loaded
resonance**, never by construction taxonomy or tonal folklore (which is usually
confounded by pickup _position_). Loading shifts every candidate's f0 down
together (√C), so rankings — and therefore assignments — are robust to cable
assumptions.

K-weighting note: a brighter template reads louder through every preset, but one
user levels all presets with one stimulus, so the common mode cancels in
relative leveling. Don't re-tune template `freq` to chase absolute loudness.

## Shipped aliases (all verified on measured-L grounds)

| Alias       | Template    | Why                                                                                                 |
| ----------- | ----------- | --------------------------------------------------------------------------------------------------- |
| Filter'Tron | Single-coil | True humbucker, but so low-wound (~1.5–2.8 H total) it sits next to a Strat coil (~2.2 H) → bright. |
| DynaSonic   | Single-coil | True bright low-wind single coil (DeArmond). Cleanest case.                                         |
| P90         | Humbucker   | True single coil, but a huge steel-pole winding (~7–8 H) resonates at/below PAF territory → dark.   |
| Gold foil   | Single-coil | Low-wind family (~4–6 kΩ); heterogeneous, but every member lands nearer SC than HB.                 |

P90 and Filter'Tron deliberately cross construction lines — that's the
methodology working, not an error.

## Vetted future candidates

- **Lipstick (Danelectro)** → Single-coil: safe.
- **Wide Range (CuNiFe)** → Humbucker: safe (CuNiFe rods keep L moderate;
  brighter than a PAF but HB-side).
- **Mini-humbucker** → coin-flip (~3.5–4 H, ~4–4.5 kHz, equidistant from both
  templates). Ship as HB only with a "closest match" framing.
- **Jazzmaster** → **NOT single-coil** despite the construction: the wide
  pancake coil measures ~3–4.5 H → loaded f0 ~3.2–3.6 kHz, closer to the HB
  template on a log axis. Coin-flip/HB. ("Warmer but still SC" is exactly the
  construction-taxonomy reasoning this doc forbids.)
- **Precision split-coil** → `bass-singlecoil` if ever surfaced, same as Jazz
  Bass. The split P is electrically a single coil that happens to hum-cancel:
  each string is sensed by ONE half (no aperture comb), the silent half sits in
  the circuit as dead series L+R, and total L (~3.5–4.5 H, DCR ~11 kΩ = two
  ~5.5 kΩ halves) lands in the same band as one J coil (~7.5–8 kΩ), nowhere
  near a true humbucker's ~2×. Since P and J share a template, prefer a label
  hint ("Bass single-coil — P, J") over two alias rows implying a difference.

## When a new topology (not an alias) would be justified

Only for a pickup family whose _spectrum_ fits none of the four guitar classes
(bright-peaky / dark-peaky / flat-active / flat-percussive) — none is known.
Output-level differences never justify one (see error budget above). A new
topology costs synth params + seed + committed WAV (`gen_samples`) and
interacts with the Doctor's threshold calibration — high bar, deliberate
decision.
