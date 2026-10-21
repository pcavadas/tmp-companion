# block-classification.json — provenance & regen recipe

`block-classification.json` holds the FenderId membership sets for the Tone Master Pro's `NodeSelectionRestrictions` block-count caps, hand-extracted from `tone-master-stomp-client` 1.8.58 and the product profile it is served [RE].
There is no build-time generator ([Why literal extraction](#why-literal-extraction-not-acdcategory--regex)).

## Why this matters

The device audio engine (`tm-stomp-server`) enforces none of these count caps: its node factory, replace, insert, save and load paths check only the CPU budget [RE].
So the companion's own guard (`src-tauri/src/blockcaps.rs`, mirrored by `src/views/copy/validateBlockEdit.ts`) is the only enforcement, and an id missing from a set lets an over-cap preset reach a real slot.
Membership is exact-string on the device model id, because ids arrive verbatim with their suffixes [RE].
Never normalize, strip a suffix or append `NoFx` before a lookup: the suffix is the classification signal.

## The caps

| Cap | Max | Form in the client |
|---|---|---|
| ConvolutionReverbLimit | 1 | hardcoded literal `std::set` (`convolutionSet`) |
| ComboHalfStackCabinetsLimit | 2 | product-profile category lookup (`cabinetSet`), one exemption (`cabinetExempt`) |
| GlooperEffectsLimit | 2 | direct compare vs one literal (`glooper`) |
| FXLoopCoexistence | — | 5×5 compatibility table over `fxLoops` |

- Convolution: `(cand∈set) − (replaced∈set) + (any member present ? 1 : 0) < 2`, so the existing term is presence, not a count [RE].
- Cabinets: each member node weighs `1 + (cabsim2enabled == true)`, an amp's second mic as much as a CabSim's second cab [HW+RE].
- The cabinet check passes while the total stays under 3, and an exempt candidate (`ACD_ExternalCab`) always passes [HW+RE].
- The host guard adds the second-cab weight only to a `cabinetSet` member: `blockcaps::Counts` ignores a `cabsim2enabled` flag on any other node.
- `cabsimCarryOver` is not a client set: it is the server's cab list (static RE, fw 1.8.58), the 68 node ids whose stock `replaceNode` into `ACD_CabSimTMS` lands dual with the old cab on both sides. Any other replaced node gives the block's on-load default, the factory single cab unless the user changed it.
- Glooper: a Glooper candidate plus the existing ones, minus a replaced one, stays under 3 [RE].
- FX loops: no loop appears twice, and the stereo `ACD_FxLoop3_4` excludes the mono `ACD_FxLoop3` and `ACD_FxLoop4` [RE].
- The client also limits loops by signal-chain line type [RE]; the companion does not mirror that part.

## Why literal extraction (not acdCategory / regex)

- `convolutionSet` is the client's literal list, which is the ground truth; on fw 1.8.58 it equals the `acdCategory ∈ {DspUnitACDConvEffect, DspUnitACDAmpCabsimReverb}` predictor (20 ids) [RE].
- `ACD_TMSuperReverbVibratoCabIRConRvb` carries a typo (`ConRvb`, not `ConvRvb`), so a `/ConvRvb$/` regex misses it [RE].
- `cabinetSet` is the product profile's `combos`, `halfstacks`, `bassAmps` and `cabinets` categories plus two client overrides, `ACD_CabSimTMS` (cabinets) and `ACD_UserIRTMS` (IR) [RE].
- The 62 standalone cab models carry an empty `acdCategory`, so an `acdCategory` predictor misses them [RE].
- `cabinetSet` also keeps 7 amp+cab server units the profile does not list (`ACD_BloomfieldDriveCabIR`, `ACD_DeluxeReverb65NormalCabIR`, `ACD_HypersonicAmp6L6{Blue,Green,Red}HalfCabIR`, `ACD_TMSuperReverbNormalCabIR`, `ACD_TwinReverb65NormalCabIR`) [RE].
- The client cannot classify those 7 [RE]; each models a cab, so the companion counts them as the fail-closed choice.

## Regen recipe (next firmware bump)

1. Convolution: read the literal inserts of the client function that builds the `NodeSelectionRestrictions` convolution set (the one referencing `ACD_TMSpring63Conv`).
2. Cabinets: take every id in the product profile's `combos`, `halfstacks`, `bassAmps` and `cabinets` categories (the `product_profile.json` the server serves), add `ACD_CabSimTMS` and `ACD_UserIRTMS`, drop the client's exemption list (`ACD_ExternalCab`), and keep any amp+cab server unit the profile omits.
3. Re-check the cabinet weight: the `+1` reads `cabsim2enabled` from the node's cab-2 sub-object.
4. Glooper and FX loops: read the single `ACD_Glooper` compare and the FX-loop compatibility table.
5. Check every id against the `tm-stomp-server` unit set (548 units on fw 1.8.58) to catch transcription errors; `ACD_FxLoop3`, `ACD_FxLoop4` and `ACD_FxLoop3_4` are profile markers, not DSP units.

Sets recorded for fw 1.8.58: 20 convolution, 133 cabinet (126 the client counts plus the 7 above), 1 exemption, 1 glooper, 5 FX loops [RE].
