# Accuracy benchmarks

Green tests prove state and flow contracts; they do not prove that the Doctor's verdicts or a leveling run are accurate on sounds the thresholds were never tuned on.
This note covers the two tools that measure that, the corpus format they read, and what the simulator cannot stand in for.
The code is `src-tauri/src/benchmark.rs` (pure) and `src-tauri/src/probe_api/benchmark.rs` (the `probe` arms); neither arm touches the device.

## The in-sample problem

The shipped guitar constants come from the 248-sound fw 1.8.58 factory sweep: the target and the stimulus reference directly, the gates rank-transferred from an earlier 25-preset calibration ([Committed baseline](#committed-baseline)).
About 13 rules at 3 playback levels compound per sound, and judged on the sweep they came from, any calibration looks better than it is; leave-one-out corrects for that.

## Leave-one-out

```text
probe --doctor-loo <factory_report.json> [--kind synthetic|capture] [--out <loo.json>]
```

It reads a `probe --doctor-calib-factory` report.
Every factory preset counts as healthy, so any verdict on a held-out preset is a false positive.
For each preset, the calibration is re-derived from the other presets and the held-out one is diagnosed with it.
Three modes re-derive progressively more:

- `targetOnly`: the target curve is re-derived; the population gates stay shipped.
- `maxPlusMargin`: the target is re-derived, and the gates become the healthy max plus a margin (1.2 dB for band rules, 0.15 dB/oct for tilt), as documented on `GUITAR`.
- `p75Consensus`: the target is re-derived, the consensus band gates (both spaces) sit at the 75th percentile as documented on `Thresholds`, and tilt and fizzy use max plus margin.

The target is re-derived the way the shipped one was authored: the per-band median of each preset's mean-removed, stimulus-anchored band dB, rounded to 0.5 dB.
No mode re-derives the `washed` gate (rank-transferred), the bass-only `buried` gate (it needs a graph) or the localized `resonant` / `boxy` gates (set from a separate injection matrix).

The report gives, per rule and for "any rule", the fire count, the rate and the 95% Wilson upper bound (0 of 25 still allows about 13%), next to the in-sample result of the shipped calibration on the same sounds.
Each report row must carry a `replay` profile: everything the engine reads from the capture (raw band powers, stimulus band powers, coverage, tail ratio, air flatness, loudness, peaks).

## Committed baseline

The held-out benchmark runs in `cargo test --lib` (`benchmark::held_out_baseline`).

- **Data:** `src-tauri/src/fixtures/doctor-guitar-factory-2026-10-15.json`, the 248 guitar-family rows of a `probe --doctor-calib-factory` sweep over factory slots 0–254, replay profiles only (floats rounded to 9 significant digits, peaks to 6; the leave-one-out result is identical to the unrounded report's).
- **Provenance** (inside the file): the owner's Tone Master Pro, fw 1.8.58, macOS host, release `probe`, the production Doctor window with a fresh connection per engage, through the shipped `guitar-humbucker.wav` (sha256 `89cebec6…bfd475`). The shipped guitar constants (`GUITAR_STIM_REF`, `GUITAR_TARGET`, the rank-transferred gates) were derived from this same sweep, so its in-sample column is in-sample by construction.
- **Labeling:** none by ear. Every factory preset counts as healthy, so every verdict is a false positive.
- **Acceptance:** `src-tauri/src/fixtures/doctor-guitar-loo-baseline.json` holds every rule's fire count per mode, and the test fails when any count grows. When a change lowers counts, re-bless with `TMP_BLESS_LOO=1 cargo test --lib held_out_baseline` and update the table below.

False positives on 248 healthy guitar sounds (count; 95% Wilson upper bound):

| Rule | Shipped (in-sample) | `maxPlusMargin` | `p75Consensus` | `targetOnly` |
|---|---|---|---|---|
| muddy | 24 (≤14.0%) | 0 (≤1.5%) | 37 (≤19.9%) | 21 (≤12.6%) |
| boomy | 5 (≤4.6%) | 0 (≤1.5%) | 36 (≤19.4%) | 8 (≤6.2%) |
| harsh | 8 (≤6.2%) | 1 (≤2.2%) | 43 (≤22.5%) | 8 (≤6.2%) |
| fizzy | 9 (≤6.8%) | 1 (≤2.2%) | 1 (≤2.2%) | 9 (≤6.8%) |
| lost | 33 (≤18.1%) | 1 (≤2.2%) | 53 (≤26.9%) | 35 (≤19.0%) |
| thin | 25 (≤14.5%) | 0 (≤1.5%) | 37 (≤19.9%) | 25 (≤14.5%) |
| dark | 17 (≤10.7%) | 1 (≤2.2%) | 1 (≤2.2%) | 17 (≤10.7%) |
| bright | 7 (≤5.7%) | 0 (≤1.5%) | 0 (≤1.5%) | 7 (≤5.7%) |
| washed | 36 (≤19.4%) | 36 (≤19.4%) | 36 (≤19.4%) | 36 (≤19.4%) |
| buried | 0 (≤1.5%) | 0 (≤1.5%) | 0 (≤1.5%) | 0 (≤1.5%) |
| resonant | 7 (≤5.7%) | 7 (≤5.7%) | 7 (≤5.7%) | 7 (≤5.7%) |
| boxy | 4 (≤4.1%) | 4 (≤4.1%) | 4 (≤4.1%) | 4 (≤4.1%) |
| any rule | 124 (≤56.2%) | 45 (≤23.4%) | 163 (≤71.4%) | 126 (≤57.0%) |

The shipped gates keep the percentile the earlier gates held (rank transfer), and on this population that percentile fires at least one rule on half of the factory guitar presets.
`washed`, `resonant` and `boxy` are re-derived by no mode, so their counts are the shipped gates' held-out rate.
Under `maxPlusMargin`, 45 of 248 (18.1%) still draw a verdict, most of them `washed` (36) and the localized `resonant` (7) and `boxy` (4).

The committed baseline measures only false positives.
Per-diagnosis precision and recall need labeled positives with a `replay` profile (the defects suite's reports carry verdicts, not replays); the leveling error distribution, false clamps and idempotence need validate-log rows from a labeled leveling run; run-to-run variance and timing need a repeated subset.
Those rows belong in a [labeled corpus](#labeled-corpus).

## Labeled corpus

```text
probe --benchmark <corpus.json> [--thresholds <thresholds.json>] --out <result.json>
```

It exits 1 when any threshold is violated; the result JSON is deterministic and names every violation.

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

Thresholds are optional per field and are set from a measured baseline, never guessed.
An unknown or misspelled key is refused, so a gate can never be silently off:

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

## Corpus coverage

A corpus should span guitar, bass and Bass VI; humbucker, single-coil and DI stimuli; clean and driven chains; wet and dry; split and parallel routing; and sounds near each threshold.
Repeat a subset of hardware captures to measure run-to-run variance, and record the warm per-sound and whole-run times alongside.

## Simulator limits

The offline `SimDevice` is not an accuracy oracle, so no benchmark row may come from it:

- Its gain model cannot represent compressors, modulation or any spectral defect.
- `sim_device::stored_from_preset` takes the first `outputLevel`-bearing node in each sound, not the routed, active amp. The e2e fixtures compensate by keeping every amp in a sound at the same `outputLevel` (pinned by `fixture_gates`), so unequal or muted parallel lanes are never exercised offline; those cases need hardware rows.
- Offline Doctor captures are synthetic transfers, not the chain's real response.
