# Held-out accuracy benchmarks (Doctor and leveling)

Green tests prove state and flow contracts. They do not prove that the Doctor's verdicts or a leveling run are accurate on sounds the thresholds were never tuned on. This note covers the two tools that measure that, the corpus format they read, and what the simulator cannot stand in for. The code is `src-tauri/src/benchmark.rs` (pure) and `src-tauri/src/probe_api/benchmark.rs` (the `probe` arms). Neither arm touches the device.

## The in-sample problem

`GUITAR_TARGET` is the median shape of the 25-preset R5 factory sweep, and the `GUITAR` gates were set on the same 25 presets (`doctor.rs`). Judged on that sweep, both look perfect by construction. With n = 25 the population maximum sits near the 96th percentile, so the false-positive rate on an unseen healthy preset is unknown, and about 13 rules at 3 playback levels compound per sound.

## Leave-one-out: `probe --doctor-loo`

```text
probe --doctor-loo <factory_report.json> [--kind synthetic|capture] [--out <loo.json>]
```

Reads a `probe --doctor-calib-factory` report. Every factory preset counts as healthy, so any verdict on a held-out preset is a false positive. For each preset, the calibration is re-derived from the other presets and the held-out one is diagnosed with it. Three modes re-derive progressively more:

| Mode            | Target curve | Population gates                                                                                                                             |
| --------------- | ------------ | -------------------------------------------------------------------------------------------------------------------------------------------- |
| `targetOnly`    | re-derived   | shipped                                                                                                                                      |
| `maxPlusMargin` | re-derived   | healthy max + the R4 margin (1.2 dB band rules, 0.15 dB/oct tilt), the derivation `GUITAR`'s doc states                                      |
| `p75Consensus`  | re-derived   | consensus band gates (both spaces) at the 75th percentile, the derivation `Thresholds`' consensus doc states; tilt and fizzy at max + margin |

The target is re-derived the way the shipped one was authored: the per-band median of each preset's mean-removed, stimulus-anchored band dB, rounded to 0.5 dB. No mode re-derives the `washed` gate (ground truth: a verified 90%-wet reverb), the bass-only `buried` gate (it needs a graph) or the localized `resonant`/`boxy` gates (set from a separate injection matrix).

The report gives, per rule and for "any rule", the fire count, the rate and the 95% Wilson upper bound (0 of 25 still allows about 13%), next to the in-sample result of the shipped calibration on the same sounds.

**Data needed.** Each report row must carry a `replay` profile: everything the engine reads from the capture (raw band powers, stimulus band powers, coverage, tail ratio, air flatness, loudness, peaks). Sweeps record it from this change on. The 2026-07-16 R5 sweep's per-preset data is not in the repo, so the first estimate needs one re-run of `probe --doctor-calib-factory` on the unit, through the shipped humbucker stimulus, at the production window.

## Labeled corpus: `probe --benchmark`

```text
probe --benchmark <corpus.json> [--thresholds <thresholds.json>] --out <result.json>
```

Exits 1 when any threshold is violated. The result JSON is deterministic and names every violation.

```jsonc
{
  "provenance": {
    "hardware": "Tone Master Pro, unit A",
    "firmware": "1.8.58",
    "stimulus": "resources/samples/guitar-humbucker.wav",
    "stimulusHash": "<the sweep report's stimulus.hash>",
    "capturePolicy": "Doctor window, fresh connection per engage, humbucker DI",
    "labeling": "two listeners, blind, by ear",
  },
  "family": "guitar",
  "stimulusKind": "synthetic", // or "capture" (the fizzy flatness gate)
  "doctor": [
    {
      "id": "factory-12",
      "split": "validation",
      "labels": ["muddy"],
      "replay": {/* a sweep row's replay */},
    },
  ],
  "leveling": [
    // A validate-log row (validate_log.rs) plus split and measured_lufs.
    {
      "label": "scene:slot404:scene2",
      "split": "validation",
      "target_lufs": -17.0,
      "measured_lufs": -17.1,
      "clamped": false,
      "expect_clamp": false,
      "persist_mismatch": false,
      "rerun_writes": 0,
    },
  ],
}
```

- **Provenance is mandatory.** A corpus with any empty provenance field is refused.
- **The split.** Rows marked `tuning` are the data a threshold may be set on; only `validation` rows are scored. A sound never moves from validation to tuning after it has graded a threshold.
- **Doctor metrics, per diagnosis:** precision, recall, false-positive rate and abstention rate. A rule abstains when the engine could not have fired it on that row, and an abstention is never counted as a negative. The gates come from `doctor::rule_judgeable`, which mirrors the engine: the rule's band was not excited, the tilt rules (and `thin`) have fewer than 3 covered body bands, `thin` on a bass family, or a localized rule with no covered band in its scan range. A replay carries no graph, so `buried` always abstains.
- **Leveling metrics:** the absolute error distribution (median, p90, max) and the mean signed error over measured, unclamped rows; false clamps (clamped while labeled reachable); persistence mismatches; re-runs that wrote again (a correct run is idempotent); unmeasured rows.

**Thresholds** are optional per field and are set from a measured baseline, never guessed. An unknown or misspelled key is refused, so a gate can never be silently off:

```json
{
  "doctor": {
    "muddy": {
      "minPrecision": 0.8,
      "minRecall": 0.7,
      "maxFpRate": 0.05,
      "maxAbstentionRate": 0.2
    }
  },
  "leveling": {
    "maxP90AbsErrorLu": 0.5,
    "maxAbsErrorLu": 1.0,
    "maxFalseClamps": 0,
    "maxPersistMismatches": 0,
    "maxNonIdempotentReruns": 0,
    "maxUnmeasured": 0
  }
}
```

A threshold whose metric is undefined (for example precision on a rule that never fired) is a violation: no evidence is not a pass.

## What a corpus should cover

Guitar, bass and Bass VI; humbucker, single-coil and DI stimuli; clean and driven chains; wet and dry; split and parallel routing; sounds near each threshold. Repeat a subset of hardware captures to measure run-to-run variance, and record the warm per-sound and whole-run times alongside.

## Simulator limits

The offline `SimDevice` is not an accuracy oracle, so no benchmark row may come from it:

- Its gain model cannot represent compressors, modulation or any spectral defect.
- `sim_device::stored_from_preset` takes the first `outputLevel`-bearing node in each sound, not the routed, active amp. The e2e fixtures compensate by keeping every amp in a sound at the same `outputLevel` (pinned by `fixture_gates`), which means unequal or muted parallel lanes are never exercised offline. Those cases need hardware rows.
- Doctor captures offline are synthetic transfers, not the chain's real response.
