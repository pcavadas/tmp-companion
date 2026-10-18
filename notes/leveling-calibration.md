# Leveling calibration and metering

Read this when changing instrument profiles, the leveling stimulus, playback-level compensation, the capture window or the loudness meter.

The model and the base lane are in [leveling.md](leveling.md).
Facts are for firmware 1.8.58 unless a line says otherwise.

## Instrument profiles

A profile links a real instrument to a shipped pickup topology ([pickup-topologies.md](pickup-topologies.md)) whose stimulus WAV is re-amped when leveling a preset the profile is assigned to.
Tier-2 calibration captures the dry instrument (K-weighted LUFS) and stores the capture as the profile's leveling stimulus (`<app_config>/captures/<profile_id>.wav`), re-amped verbatim with no scaling (`calibration_lufs` is ignored when the capture resolves).
Even the pitched-pluck stimulus mis-levels presets against a real DI by up to ~1.8 LU spread, because crest, spectrum and harmonic structure drive nonlinear presets, not integrated LUFS (`probe --stim-ab`) [HW].
A calibration without a capture falls back to the topology WAV scaled to the stored loudness.
`store_capture` writes temp + rename before the scalar is saved.
A take within the clip severity rule is stored; a worse one stores nothing and never deletes the earlier capture, and when an earlier capture exists the scalar stays too.
Settings shows on each instrument whether it levels with its recording or the test signal (`get_store` fills `Profile::has_capture`), and `save_profiles` unlinks captures on profile delete or topology change.
An activity-ratio gate (500 ms windows, ≥ 50% within 30 dB of the loudest) rejects mostly silent takes.

The dry instrument is tapped on USB-Out 3 (`audio::DRY_INSTRUMENT_IN_CH`).
`resolve_stimulus`'s precedence is: e2e env → explicit → profile capture → topology WAV → env → default.

**The USB 3 strip.**
The send defaults to PRE (fader-independent).
`calibrate_profile` reads the strip (`backup_read::usb3_strip`) from the startup settings snapshot, which can be stale, so it acts asymmetrically: a muted strip only explains a silent take (with the on-unit fix), and POST with an off-unity fader refuses a take that landed, which would otherwise drive every later re-amp at the fader-scaled level.
The app never writes the mixer.

**K-weighted.**
Peak is pick-transient-dominated and barely tracks output; flat RMS under-counts bright pickups (a bright guitar that drives the preamp hard reads low in broadband RMS).
K-weighting matches perceived drive and puts input and output on one metric.

**Clipping is a user-facing instruction.**
The dry USB 3 send is the instrument ADC pair, averaged, times a fixed 3.96 (+11.95 dB) trim, saturated to 24 bit [RE], so hard picks from a normal passive guitar pin it while the guitar, the ADC and the preset input are not overloaded; the unit's USB 3 meter is windowed RMS and does not show it ([open](open-questions.md#instrument-pad-and-the-dry-clamp)).
`calibrate_profile` flags a take with a flat-top run-length detector (≥ 4 consecutive full-scale samples, `lufs::is_clipped`; a lone transient at full scale is not clipping), and storage follows `stimulus::clip_verdict`: Mild (≤ 0.5% of samples at full scale, longest run ≤ 2 ms) is stored, Severe is not.
Play exactly as you really play and never roll the guitar volume back: that under-drives the amp and darkens the tone (volume-pot HF rolloff), corrupting the brightness K-weighting exists to capture.
The too-quiet case is guarded separately.

The dry instrument is capturable (`probe --capture-input`), and the re-amp inject is not AGC'd: its amplitude drives the chain, and the apparent compression is the amp's own nonlinearity (`probe --agc-test <clean-slot>` linearizes at low drive; a clean Twin gave ~1.65 LU output per 6 dB input) [HW].
Re-validate with those two probe arms.

## Playback-level compensation

Equal LUFS is equal loudness only near one SPL.
The store carries a playback level (Quiet / Rehearsal / Stage, default Stage), and `profiles::playback_offset_lu` adds a per-family LU offset to the target: bass and Bass VI get Quiet 0, Rehearsal −1.0, Stage −1.5 LU; guitar is 0 everywhere.

Quiet is the anchor because K-weighting was fitted to listening tests run at about 60 dBA: ITU-R BS.1770-5, Annex 1, Attachment 1, §4.1 describes them (Benjamin 2004, AES preprint 6233; Soulodre 2004, "Evaluation of objective measures of loudness", _Canadian Acoustics_ 32(3), pp. 152–153: 97 subjects, the reference reproduced at 60 dBA).
No source makes stage volume its design point.
So equal LUFS is equal loudness near bedroom level, and the ISO 226 contours flatten as playback gets louder: the ear credits low-frequency energy more than K-weighting does, and a bass preset at equal LUFS sounds bigger.
Bass therefore gets a negative offset that grows with level.
The magnitudes are deliberately coarse: contours vary by listener and rig, and a bass preset's loudness shift depends on how much of its energy sits below ~200 Hz.
The Doctor's threshold offsets use the same contours, derived per band ([doctor-thresholds.md#playback-level-offsets](doctor-thresholds.md#playback-level-offsets)).

## Capture window

The leveling default is the full capture (`audio::MeasureOpts::full`), because the window is load-bearing ([device/reamp-audio.md#capture-window](device/reamp-audio.md#capture-window)).
The adaptive early-exit capture (`audio::MeasureOpts::adaptive`) is opt-in only; adopting it is a re-baseline and a product decision, not a drop-in speedup.
A steady-state window did not beat the current window against real-DI playing on 12 presets (median error 1.60 vs 1.50 LU); the stimulus spectrum moves the error ~10× more [HW].
Conclude a bias only from a many-sample A/B against the full-capture oracle ([leveling.md#re-validating-a-timing-change](leveling.md#re-validating-a-timing-change)).

## Metering convention

Companion measures the processed USB pair as standard 2-channel BS.1770 (`lufs::measure_stereo`, an energy sum over capture channels 0 and 1, the dry channel 2+ excluded), matching ffmpeg's independent `ebur128` within 0.009 LU.
On the unit's mirrored dual-mono USB output (`ch0 ≡ ch1`) this reads exactly +3.0103 dB (`10·log10(2)`) above a loudest-single-channel meter (measured +3.010 LU) [HW].
Stores saved under the single-channel convention are migrated once (`profiles::Store::meter_convention`), shifting their targets +3 LU to keep the same audible loudness; relative quantities (spreads, deltas, the `presetLevel` solve) are invariant to the convention.
The input side (self-measurement, dry-DI calibration, the floor-guard spread) uses `lufs::measure_mono`.
The dev Doctor probe arm (`probe --doctor`) reports mono-on-mixdown loudness (`SoundProfile::from_capture_with_psd_loudness` with `stereo_loudness: None`); the production Doctor path (`commands/doctor.rs` → `leveller::doctor_capture_with_loudness`) reports the stereo measure.

### Playback outputs

A rig that plays the unit in mono (one cab, or a mono PA fed L+R) or from output 1 alone hears a different signal whenever a preset is genuinely stereo (a ping-pong delay, a wide reverb, a stereo split).
Settings → Playback → Outputs (`profiles::Store::playback_channels`, default `stereo`; a store without the field loads as `stereo`) picks what the leveling meter hears (`audio::metered_frame`):

- `stereo`: the 2-channel BS.1770 power sum over USB 1/2, as in [Metering convention](#metering-convention).
- `mono_sum`: `(L+R)/2` metered as mono, +3.0103 dB.
- `left_only`: `L` metered as mono, +3.0103 dB.

The +3.0103 dB is applied by putting the one signal on both meter channels, so the gates run on the same shifted scale and a dual-mono capture reads bit-identically in all three modes: the default and every mirrored-mono preset change nothing, and targets mean the same loudness in every mode.
Only decorrelated stereo differs: uncorrelated equal-power L/R reads 3.01 dB lower summed than in stereo, and left-only reads the left side's own level.
The choice is process-wide (`audio::meter_channels`, synced from the store at startup and by `set_playback_channels`) and reaches every output-side hub: `leveller::measure_processed` (every full-capture leveling measurement) and the streaming `audio::PairMeter` behind the live-LUFS readout and the adaptive `reamp_measure`.
The external-validation WAV dump (`validate_log`, `probe --dump-wav`) writes the pair as the meter heard it, so the ffmpeg judge grades the same signal.
The Doctor is unaffected: its analysis capture pins the stereo meter (`leveller::doctor_capture_with_loudness`), and its band and PSD analysis stays on the averaged mixdown.
Gates: `leveller::playback_channels_meter_tests`, `profiles::tests::playback_channels_default_stereo_and_round_trip`, `SettingsView.test.tsx` ("playback outputs").

### Parity harness

`scripts/meter-parity.sh` checks the metering convention against an independent BS.1770 implementation, ffmpeg's `ebur128` filter.
It is a developer tool, not a gate: not wired into `scripts/gates.sh`, never a CI dependency, and ffmpeg is never linked into or invoked by the shipped app.

```bash
bash scripts/meter-parity.sh                 # generate deterministic synthetic fixtures via ffmpeg lavfi and compare
bash scripts/meter-parity.sh <dir-of-wavs>   # compare an existing directory, e.g. real captures from `probe --dump-wav`
```

Both arms run the same WAV through two meters, `probe --measure-wav` (`lufs.rs`'s production output-side path: `measure_stereo` for a ≥ 2-channel file, `measure_mono` for 1 channel) and ffmpeg's `-af ebur128`, and assert `|Δ integrated| ≤ 0.02 LU` per file.
The ffmpeg side is read at full precision from the filter's metadata (`ebur128=metadata=1` + `ametadata=print:key=lavfi.r128.I`, the last value emitted), not from the end-of-run summary, which prints one decimal.
Agreement is ≤ 0.009 LU across the six synthetic fixtures and 0.004–0.006 LU on real captures; two conformant implementations differ slightly in how they handle the final partial gating block, so 0.02 is a tight but achievable bar.
Without `ffmpeg` on `PATH` the script prints a skip notice and exits 0.
A FAIL means `lufs.rs` disagrees with ffmpeg by more than 0.02 LU on at least one file; treat it as a `lufs.rs` regression first (channel or hop handling, most likely the 2-channel true-peak-per-channel or the frame-vs-sample hop math its header documents), unless the file is a real capture, where device noise or a DC offset is also plausible.

`probe --dump-wav <dir>`, passed alongside `probe --measure-current` or `--measure-scene`, also writes the captured processed pair (never the dry channel) to a 32-bit-float WAV in `<dir>`, named from slot and scene, and changes nothing else.
Re-running the same slot and scene overwrites the earlier dump.
