# E2E fixtures

The scenario fixtures are the eleven presets in `e2e/fixtures/scenario-presets.json` (list indices 400–410).
Which fixture carries which use case is the matrix in [`e2e/fixtures/COVERAGE.md`](../e2e/fixtures/COVERAGE.md), which `fixture_gates.rs` parses; this note holds the rules those fixtures are authored under.
Read both before changing a fixture, and update them in the same commit.

## Tiers

`Offline` means the SimDevice capture model can reproduce the case (`sim_device.rs::model_lufs` plus `scenario-loudness.json`).
`Online` means only the real device can: the offline model has no lane summation and no measured clamp verdict for a scene knob it cannot scale.
A spec written offline against an `Online`-only row will not pass; write it under the online tier.
SimDevice implements `setFootswitchAssignment` (54) and `clearFootswitchAssignment` (55).

Loudness accuracy on the `Online` rows, as opposed to the flow every row tests, is checked by external validation (`scripts/level-validate.sh`).
Each strict re-measure writes its own capture to a WAV plus one expectation row (`src-tauri/src/validate_log.rs`), and after the online spec loop passes, `scripts/e2e.sh` judges those WAVs with ffmpeg's `ebur128`, independent of the repo's own `lufs.rs`.
Base, scene and footswitch sounds are all covered (`probe --measure-footswitch` for the footswitch captures); the Doctor writes nothing, so it has no write path to validate.
The gate semantics (advisory while ffmpeg is absent, a real gate once it is present and rows were emitted) and the 0/1/3 exit contract are in `.claude/rules/e2e.md`'s "External validation" section; `scripts/validate-hbe.sh` is the attended standalone run on a Friedman HBE.

## The cab rule

Every guitar amp in every fixture is a combo, an amp+cab merged model (a cab- or IR-suffixed id), or a bare head with a cabinet block downstream in its own lane.
`fixture_gates::every_guitar_amp_in_every_fixture_reaches_a_cab` enforces it by walking every complete signal path the production routing decoder produces, so a head in the pre-split trunk with a cab in each parallel lane (404's shape) passes, and a head with a cab in a lane it never reaches does not.
How each fixture meets it is the cab-rule table in `COVERAGE.md`.

## Authoring constraints

1. **Every amp `outputLevel` inside one sound is equal.**
   The offline model's stored-level probe (`sim_device::stored_from_preset`) takes the first `guitarNodes` node (scanning `G1..G7`) that carries an `outputLevel`, not the unbypassed amp of the sound under measurement, so two amps at different values would make the modelled `ol_term` depend on map ordering. 400 keeps both amps in lockstep per scene, and 403 keeps both lane amps at the same value.
2. **403's amps live outside `G1`**, in the split lanes, and the probe finds the first lane amp: `1.0`, exactly what both amps are authored at, except the deliberate zero-authority scene, whose `0.0` the model renders as silence.
   Written-over-stored is therefore faithful, and the closed-form joint-k solve converges in one step.
3. **Scene overlays follow the device's overlay shapes** (`scene_jobs::scene_overlay`): Scene Edit enabled ⇒ a full overlay, Scene Edit disabled ⇒ a bypass-only overlay (`{bypass, bypassType}`), never materialized ⇒ no entry.
   All three appear in the set (matrix rows 10–12).
4. **Each fixture stays under 16 KiB of serialized `presetJson`** (`fixture_gates::e2e_fixtures_stay_inside_the_field8_read_budget`).
   `E2E Hiwatt 3S` is the one exemption, a verbatim device export whose exact size is pinned instead.
   A per-scene `splitMix` alone costs ~730 B and is mandatory, so on 402 every extra overlay is paid eight times; its scenes carry one overlay each at most.
5. **Names keep the `E2E ` prefix.**
   `e2e_seed::scenario_strays` matches scenario names to find aborted-seed leftovers, and every fixture carries `info.source_id == FIXTURE_SOURCE_STAMP` plus `e2e00000-`-prefixed scene uuids as the ownership markers the seed and teardown guards read.
   Bump the stamp's `#rN` suffix on every fixture regeneration.

Constraints 1 and 2 exist only because `stored_from_preset` picks the first `outputLevel`-carrying node; selecting the unbypassed amp instead (`rendered_bypass` already exists for this) would retire both, with their gate assertion.

## Deliberate deviations

- **No real chorus or delay block.**
  The committed corpus has no device-exported `dspUnitParameters` map for one, and inventing a parameter shape is what the "derive from real preset JSON" rule forbids.
  `ACD_TMSpring63` (a real map, a real `mix`) carries the wet-mix rows, and `ACD_TremoloBias` (real `noteDivision` / `tapTimeBPM`) carries the tempo-sync row.
  A real delay map captured off the unit belongs in 401.
- **The wet-mix loudness curve is authored, not measured.**
  `ACD_TMSpring63`'s parameter map is real, but no wet-mix sweep has been captured, so row 18's offline response (`sim_device::wet_mix_gain_db`: `20·log10(1 + mix)`, a monotone +6.02 dB dry → full-wet stand-in) is authored.
  It is a delta anchored on the authored mix and exactly zero unless a run sweeps the parameter, so it perturbs no other row; an offline wet solve proves the outcome plumbing, never the real preset's numbers.
- **`ACD_Boost`'s parameter map is the one hand-authored node** (`{bypass, bypassType, gain}`).
  Its `gain` semantics are hardware-verified (`param_class.rs`: base 2.5 = +2.5 dB, writes of 0 / 2.5 / 5 / 7 accepted, loudness tracked ~1:1), but the surrounding parameter set was not captured; the device fills anything missing on import.
- **A wah stands in for "a phaser with only non-level params".**
  `ACD_CryBabyQ535` is the only block in the corpus whose entire real parameter map classifies `Other`, which is the structural fact the Other-class rows need (row 19).
- **`buried` has no defect switch.**
  The Doctor's `buried` check fires only for `Family::Bass | BassVi` with a drive detected, both impossible on 407's guitar-topology preset.
- **`trade_floor` and `partial_trade` (`ClampKind`) have no fixture and are not reachable offline.**
  Both describe a headroom trade that failed mid-flight (the base fader pinning its floor while holding base on target, or a dependent write not landing so the pair backs out), and no fixture authors a trade that lands short.
  They are pinned in Rust only, by `e2e_server_tests.rs`'s `a_cancel_after_a_landed_trade_returns_its_outcomes_with_the_trade_disclosed` and `headroom_trade.rs`'s own unit tests.
  The UI shows one generic clamp message for every cause, so a future reachable fixture needs no frontend change.
