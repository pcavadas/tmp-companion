# Pickup topologies and aliases

Reference for `src-tauri/src/topologies.rs` (`TOPOLOGIES` and `ALIASES`).
Read it before adding an alias or a new topology.

## What a template encodes

A topology is a synth recipe for the leveling stimulus (`gen_samples`), not a pickup model.
Every stimulus plays the same musical phrase for its instrument (open chords and single-note runs for guitar, an eighth-note line for bass, the guitar phrase an octave down for Bass VI) on Karplus–Strong plucked strings.
It then passes through a resonant 2nd-order low-pass, the pickup's loaded resonance.
Four parameters matter for assignment:

- **`freq`**: the low-pass corner, the pickup's loaded electrical resonance (coil L plus cable and pot C): humbucker 2.8 kHz, single-coil 4.5 kHz, bass humbucker 2.5 kHz. Active and piezo pickups are low-impedance, so their corner sits high (5–9 kHz) with no hump.
- **`q`**: resonance height, 1.8–2.8 for passive coils and ≤0.9 for active and piezo.
- **`pick_softness`**: the one-pole pole on each string's excitation; a higher value rounds the attack.
- **`peak`**: output level, which is amp-input drive for uncalibrated profiles.

A real DI carries ≲2% of its power above 2.5 kHz [HW], and `every_stimulus_has_a_pickup_like_spectrum` keeps every committed WAV under 3% there.

Hum-cancelling appears nowhere.
Construction (single-coil vs humbucker) and voicing are independent axes; templates follow voicing.

## Assignment method

Three measured facts set the error budget:

1. **Synthetic vs real.** The LUFS-matched synthetic stimulus mis-levels presets against a real DI by a ~1.8 LU (humbucker) / ~1.2 LU (single-coil) spread (`probe --stim-ab`) [HW]. Spectral distinctions within a family are second-order against this; Tier-2 calibration (the instrument's own DI capture) is the real fix.
2. **Capture noise.** ~0.12 LU run to run on non-stationary presets [HW].
3. **Amp compression.** ~1.65 LU per 6 dB of input drive on a clean Twin (`probe --agc-test`) [HW], so a 0.05 `peak` delta (~0.9 dB) moves the result ~0.25 LU, at the noise floor. `peak` deltas between similar pickups are ignorable. Calibration does not absorb them: it replaces the stimulus entirely, and templates only serve uncalibrated profiles.

So an alias only has to land the gross brightness class (bright single-coil ~4.5 kHz, dark humbucker ~2.8 kHz, flat active), judged by measured inductance → loaded resonance, never by construction taxonomy or tonal folklore, which pickup position usually confounds.
Cable loading shifts every candidate's f0 down together (√C), so rankings, and therefore assignments, are robust to cable assumptions.

A brighter template reads louder through every preset under K-weighting, but one user levels all presets with one stimulus, so the common mode cancels in relative leveling.
Do not re-tune a template's `freq` to chase absolute loudness.

## Shipped aliases

All four are assigned on measured inductance:

- **Filter'Tron → Single-coil.** A true humbucker, but so low-wound (~1.5–2.8 H total) that it sits next to a Strat coil (~2.2 H): bright.
- **DynaSonic → Single-coil.** A true bright, low-wind single coil (DeArmond).
- **P90 → Humbucker.** A true single coil, but a large steel-pole winding (~7–8 H) resonates at or below PAF territory: dark.
- **Gold foil → Single-coil.** A heterogeneous low-wind family (~4–6 kΩ), every member nearer single-coil than humbucker.

P90 and Filter'Tron deliberately cross construction lines: that is the method working.

## Candidate aliases

- **Lipstick (Danelectro) → Single-coil:** safe.
- **Wide Range (CuNiFe) → Humbucker:** safe; the CuNiFe rods keep L moderate, brighter than a PAF but on the humbucker side.
- **Mini-humbucker:** a coin-flip (~3.5–4 H, ~4–4.5 kHz, equidistant from both templates); ship as humbucker only with a "closest match" framing.
- **Jazzmaster → Humbucker side, not single-coil**, despite its construction: the wide pancake coil measures ~3–4.5 H, a loaded f0 of ~3.2–3.6 kHz, closer to the humbucker template on a log axis.
- **Precision split-coil → `bass-singlecoil`**, the same as Jazz Bass. Each string is sensed by one half (no aperture comb), the silent half sits in the circuit as dead series L+R, and the total L (~3.5–4.5 H, DCR ~11 kΩ as two ~5.5 kΩ halves) lands in the same band as one J coil (~7.5–8 kΩ), nowhere near a true humbucker's ~2×. Since P and J share a template, prefer a label hint ("Bass single-coil — P, J") over two alias rows that imply a difference.

## New topologies

A new topology (not an alias) is justified only for a pickup family whose spectrum fits none of the four guitar classes (bright-peaky, dark-peaky, flat-active, flat-percussive); none is known.
Output-level differences never justify one ([Assignment method](#assignment-method)).
A new topology costs synth parameters, a seed and a committed WAV (`gen_samples`), and it interacts with the Doctor's threshold calibration.
