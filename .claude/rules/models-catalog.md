---
paths:
  - "src/models/**"
  # The catalog skill also governs the block-art engine, so the rule must load there too.
  - "src/ui/blockart/**"
---

# Catalog and block-art rules

Applies while editing `src/models/`.
The full data contract is the `tmp-companion-catalog` skill; these are the invariants that break silently.

- **`blockArt.ts` (and `blockArtCatalog/`) must NOT import `catalog.ts` or `cpu.ts`.**
  Either import closes the cycle `blockArt → catalog → cpu → blockArt`, a module-init TDZ "cannot access before initialization" crash.
  The FenderId suffix rule all three need lives in the zero-import leaf `deviceId.ts` (`resolveDeviceId`), which must stay import-free.
  Cross-cutting form+art decisions resolve at the VIEW call site (which may import both), never inside a core model module.
  **This is enforced** by `@typescript-eslint/no-restricted-imports` in `eslint.config.js` — if the rule fires, fix the direction, don't disable it.
- **The catalog is GENERATED.**
  A row-data change (form, category, glyph source) must ALSO edit the generator source, or the next pipeline regen silently reverts the JSON edit **and** breaks `models-catalog.test.tsx`'s count oracles.
- **`model-display-names.json` is GENERATED too** — by `bun scripts/catalog-display-names.ts`.
  Change its rules or its acronym/keep list in the script and re-run; never hand-edit the JSON (`catalogDisplayNames.test.ts` fails on drift).
- **Null-`bid` blocks need the name fallback.**
  `resolveBlockArt(bid)` keys art by FenderId; rows with `block_id=null` (the 7 Microphones) have no `bid` and fall through to the generic pedal glyph unless resolved by NAME via `resolveBlockArtByName`.
  Any new null-`bid` block needs the name fallback **plus** a row in the coverage test.
- **Key by `(block_id, form)`** — `block_id` is not unique across form.
- **Amp-id matching is CHECK-FIRST, then strip one suffix at a time.**
  A discovered amp can carry merged cab, IR or convolution suffixes the catalog id lacks (`ACD_HiwattDR103CanModCabIR` → `ACD_HiwattDR103CanMod`), and a miss silently skips that amp's footswitch and scene leveling.
  Reverb amps are catalogued WITH the suffix (`ACD_PrincetonReverb68CabIRConvRvb`), so check the id before every strip and never greedy-strip.
  The suffix set is `ConvRvb|CabIR|NoCab|Cab|IR`; `NoFx` is part of base ids and never stripped, but after the last strip the resolver tries appending `NoFx` once (`…BlondeVibratoCabIRConvRvb` → `…BlondeVibratoNoFx`).
  Every TS lookup (`resolveBlockArt`, `cpuForBid`, `isComboBid`, …) goes through `resolveDeviceId` in the zero-import `deviceId.ts`; keep it in lockstep with Rust `scene_jobs::is_amp_model_id`.
- **The half-stack create decision is DEVICE-DRIVEN** — it keys on `cabsimid` presence; catalog `form` is used only to SUPPRESS the stack for combo-form amps.
  Keying creation on `form` draws phantom cabs onto bare heads.
- **A combo is a device id that carries its cab.**
  `isComboBid` needs the id on the device's cabinet list (`block-classification.json` `cabinetSet`, `ACD_TweedDeluxeCabIR`); the bare id a combo row also names (`ACD_TweedDeluxe`) is the amp-only model, and `nodeTileArt` draws it on the head chassis.
- **`isCombo` is a REQUIRED argument to `nodeTileArt`, with no default** — so a new caller cannot silently re-stack a combo whose modeled built-in speaker is also a `cabsimid`.
- **Art is chosen by the explicit `variant`, never by caption text.**
  A per-model treatment is an `ArtVariant` keyed by FenderId in `blockArtCatalog/variants.ts`, threaded through `BlockArt`'s `variant` prop; every BlockArt caller must pass it alongside the other art props.
