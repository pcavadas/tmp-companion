# block-classification.json — provenance & regen recipe

`block-classification.json` holds the fenderId membership sets for the Tone Master Pro's
`NodeSelectionRestrictions` block-count caps. It is **hand-extracted, byte-exact, from the device
firmware** (`tone-master-stomp-client` 1.8.45) — there is no build-time generator, because the sets
are hardcoded literal lists in the binary, not derivable from a category field (see "Why literal
extraction" below).

## Why this matters

The device audio engine (`tm-stomp-server`) enforces **none** of these caps and cannot reject an
over-cap edit (it never populates a `PresetErrorMessage`; the node factory / replace-insert core /
save-persist / dispatch / DSP-instantiation paths are all count-free — verified by two independent
decompile passes + an objdump control-flow scan). So the companion's own guard
(`src-tauri/src/blockcaps.rs`, mirrored by `src/views/copy/validateBlockEdit.ts`) is the **sole**
enforcement. Under-including an id ⇒ an over-cap preset gets written to a real slot. Keep the sets
exact and complete.

Membership is **exact-string** on the device model id (ids arrive verbatim, suffixes intact). Never
normalize / strip suffixes / append `NoFx` before lookup — the suffix is the classification signal.

## The three caps (client fns, fw 1.8.45 → 1.7.75)

| Cap                         | Max | 1.8.45 fn      | 1.7.75 fn      | Form                                                                                |
| --------------------------- | --- | -------------- | -------------- | ----------------------------------------------------------------------------------- |
| ConvolutionReverbLimit      | 1   | `FUN_00952a50` | `FUN_008d0e40` | hardcoded `QString` literal `std::set`                                              |
| ComboHalfStackCabinetsLimit | 2   | `FUN_009521a0` | `FUN_008d02f0` | category-enum classifier + explicit id set (`FUN_00952000`, set src `FUN_00523ed0`) |
| GlooperEffectsLimit         | 2   | `FUN_00951480` | `FUN_008cf9a0` | direct compare vs one literal                                                       |

Formula (all three): `(candidate∈set) − (replaced∈set on a replace) + existing < MAX+1`. A dual-cab
node (`cab_sim2_enabled`) counts as **2** toward the cabinet cap. FXLoopCoexistence: the stereo
`ACD_FxLoop3_4` is mutually exclusive (both directions) with mono `ACD_FxLoop3`/`ACD_FxLoop4`.

## Why literal extraction (not acdCategory / regex)

- The client hardcodes **19** convolution ids, but an `acdCategory ∈ {DspUnitACDConvEffect,
DspUnitACDAmpCabsimReverb}` filter predicts **20**: `ACD_PrincetonReverb68CabIRConvRvb` is
  category-tagged but **omitted** from the client's enforced list (a Fender list-maintenance gap). A
  derived filter would silently over-enforce it.
- `ACD_TMSuperReverbVibratoCabIRConRvb` has a typo (`ConRvb`, not `ConvRvb`) — a `/ConvRvb$/` regex
  misses it.
- Standalone `ACD_CabSimTMS` + `ACD_UserIRTMS` have empty `acdCategory` — only the explicit
  is-cabinet set catches them.

## Regen recipe (next firmware bump)

1. From `tm-stomp-server` strings build `fenderId→acdCategory` (brace-balanced scan of the
   `{"FenderId":"…","info":{…"acdCategory":"…"}}` blobs) as a **predictor**: conv ≈
   `{DspUnitACDConvEffect, DspUnitACDAmpCabsimReverb}`; cabinet ≈ `{DspUnitACDAmpCabsim,
DspUnitACDAmpCabsimReverb}` + `{ACD_ExternalCab, ACD_CabSimTMS, ACD_UserIRTMS}`.
2. **Verify conv against the client binary literal builder** (headless Ghidra; the fn referencing
   `ACD_TMSpring63Conv` in the `NodeSelectionRestrictions` cluster) — the literal list is ground
   truth and diverges from the predictor (19 vs 20 on 1.8.45). Read the exact `"ACD_…"` inserts.
3. Glooper: grep the client for the single `ACD_Glooper` compare.
4. Cabinet standalone `ACD_ExternalCab` is the only member of the explicit is-cabinet literal set; the
   rest come from the classifier (category-derived). The classifier enum→name table was not resolved
   byte-exact, so `ACD_CabSimTMS`/`ACD_UserIRTMS` inclusion is validated on real hardware (add 2
   combos + a standalone cab → the 3rd cab must be blocked).
5. Every extracted id must exist in the `tm-stomp-server` unit set (533 units in 1.8.45) — validate
   membership to catch transcription errors. FX-loop ids are markers, not DSP units (string-present
   only).

Sets recorded for 1.8.45: 19 convolution, 72 cabinet (69 amp+cab + 3 standalones), 1 glooper.
