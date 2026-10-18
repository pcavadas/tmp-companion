# block-classification.json — provenance & regen recipe

`block-classification.json` holds the fenderId membership sets for the Tone Master Pro's
`NodeSelectionRestrictions` block-count caps. It is **hand-extracted from the device
firmware** (`tone-master-stomp-client` 1.8.58 and the product profile it is served) — there is no
build-time generator (see "Why literal extraction" below).

## Why this matters

The device audio engine (`tm-stomp-server`) enforces **none** of these count caps: its node
factory, replace/insert, save and load paths check only the CPU budget (an over-budget replace gets
`presetError` 14, an over-budget insert aborts the server) (static RE, fw 1.8.58). So the companion's own guard
(`src-tauri/src/blockcaps.rs`, mirrored by `src/views/copy/validateBlockEdit.ts`) is the **sole**
enforcement. Under-including an id ⇒ an over-cap preset gets written to a real slot. Keep the sets
exact and complete.

Membership is **exact-string** on the device model id (ids arrive verbatim, suffixes intact). Never
normalize / strip suffixes / append `NoFx` before lookup — the suffix is the classification signal.

## The caps

| Cap | Max | Form in the client |
|---|---|---|
| ConvolutionReverbLimit | 1 | hardcoded literal `std::set` (`convolutionSet`) |
| ComboHalfStackCabinetsLimit | 2 | product-profile category lookup (`cabinetSet`), one exemption (`cabinetExempt`) |
| GlooperEffectsLimit | 2 | direct compare vs one literal (`glooper`) |
| FXLoopCoexistence | — | 5×5 compatibility table over `fxLoops` |

- Convolution: `(cand∈set) − (replaced∈set) + (any member present ? 1 : 0) < 2` — the existing term
  is presence, not a count.
- Cabinets: each member node weighs `1 + (cabsim2enabled == true)` on ANY node (an amp's second mic,
  a CabSim's second cab); a dual candidate weighs 2, a dual replaced node frees 2; pass while the
  total is under 3; an exempt candidate (`ACD_ExternalCab`) always passes.
- Glooper: a Glooper candidate plus the existing ones, minus a replaced one, under 3.
- FX loops: no loop twice, and the stereo `ACD_FxLoop3_4` excludes the mono `ACD_FxLoop3` /
  `ACD_FxLoop4`. The client also limits loops by signal-chain line type; the companion does not
  mirror that part.

## Why literal extraction (not acdCategory / regex)

- `convolutionSet` is the client's literal list; on 1.8.58 it equals the
  `acdCategory ∈ {DspUnitACDConvEffect, DspUnitACDAmpCabsimReverb}` predictor (20), but that has not
  held on every firmware, so the literal list stays ground truth.
- `ACD_TMSuperReverbVibratoCabIRConRvb` has a typo (`ConRvb`, not `ConvRvb`) — a `/ConvRvb$/` regex
  misses it.
- `cabinetSet` comes from the product profile's `combos`, `halfstacks`, `bassAmps` and `cabinets`
  categories plus two client overrides (`ACD_CabSimTMS` → cabinets, `ACD_UserIRTMS` → IR). The 62
  standalone cab models carry an empty `acdCategory`, so an `acdCategory` predictor misses them.
- It also keeps 7 amp+cab server units the 1.8.58 profile does not list (`ACD_BloomfieldDriveCabIR`,
  `ACD_DeluxeReverb65NormalCabIR`, `ACD_HypersonicAmp6L6{Blue,Green,Red}HalfCabIR`,
  `ACD_TMSuperReverbNormalCabIR`, `ACD_TwinReverb65NormalCabIR`): the client cannot classify them,
  but each models a cab, so counting them is the fail-closed choice.

## Regen recipe (next firmware bump)

1. Convolution: read the literal inserts of the client function that builds the
   `NodeSelectionRestrictions` convolution set (the one referencing `ACD_TMSpring63Conv`).
2. Cabinets: take every id in the product profile's `combos`, `halfstacks`, `bassAmps` and `cabinets`
   categories (`/home/root/tm-stomp-product-profile/product_profile.json`), add `ACD_CabSimTMS` and
   `ACD_UserIRTMS`, drop the client's exemption list (`ACD_ExternalCab`), and keep any amp+cab
   server unit the profile omits.
3. Re-check the cabinet weight: the `+1` reads `cabsim2enabled` from the node's cab-2 sub-object.
4. Glooper and FX loops: the single `ACD_Glooper` compare and the FX-loop compatibility table.
5. Every id must exist in the `tm-stomp-server` unit set (548 units in 1.8.58) — validate
   membership to catch transcription errors. FX-loop ids `ACD_FxLoop3`/`4`/`3_4` are profile markers,
   not DSP units.

Sets recorded for 1.8.58: 20 convolution, 133 cabinet (126 the client counts + the 7 above),
1 exemption, 1 glooper, 5 FX loops.
