---
name: tmp-companion-catalog
description: "The data contract for the TMP Companion model catalog and block art. Use whenever editing `src/models/` — adding or changing a catalog row, touching block artwork (`blockArt.ts` / `ui/blockart`), the CPU-cost table, or the amp-id matching logic — or when reviewing a change to `tmp-model-guide.json` or a `*.generated.ts` file. Covers the four load-bearing invariants that break silently rather than loudly: catalog generation, block-id keying, amp-id suffix collapse, and art fallback for blocks with no block_id."
---

# TMP Companion catalog

The Catalog tab and the signal-chain strip render from a shipped model catalog. This skill is the data contract for editing it without introducing a change that passes locally but is wrong.

**Orient first.** [`.claude/rules/models-catalog.md`](../../rules/models-catalog.md) carries the edit-time invariants for `src/models/` and loads automatically when you open one of those files; this skill is the longer checklist. `CLAUDE.md` is the index and wins on any rule it states — tell the user if you spot drift.

## The four invariants (each breaks silently)

1. **Generated output — never hand-edit.** `src/models/tmp-model-guide.json`, `src/ui/blockart/blockColors.generated.ts`, and `src/models/cabCovering.generated.ts` are produced offline by generator scripts (`pipeline.py`, `expand_catalog.py`, `colorcheck.py`) that live only on the maintainer's machine, not in this repo. A taxonomy/row edit to the JSON or a `*.generated.ts` file alone is **silently reverted on the next regen** _and_ breaks the count oracle in `models-catalog.test.tsx`. A JSON-only diff is a review smell: flag the maintainer so the local generator gets mirrored — don't assume you can regenerate it yourself. (Only `scripts/extract_cab_covering.py` — which regenerates `cabCovering.generated.ts` from the extracted client binary passed as `argv[1]` — ships in this repo; the main catalog pipeline does not.) **The frontend does not import the full guide:** `catalog.ts` reads `src/models/tmp-model-guide.slim.json`, which `scripts/slim_model_guide.py` projects from the full guide onto the fields `RawBlock` reads (dropping the vendor `image` tile paths and the provenance fields). Re-run it after ANY change to the full guide (a regen, `extract_strip_names.py`); `models-catalog.test.tsx` fails on drift. A new field `catalog.ts` reads must also join the script's `KEEP` list. Rust still embeds the FULL guide (`scene_jobs.rs`, `block_id` + `category`). **The names the Catalog shows** come from a third generated file, `src/models/model-display-names.json` (the unit's capitals → normal case, "FENDER '57 DELUXE" → "Fender '57 Deluxe"), written by `bun scripts/catalog-display-names.ts` from the slim guide; re-run it after the slim guide changes. Its rules and its explicit acronym/keep list live in the script, never in the JSON; `catalogDisplayNames.test.ts` fails when a model has no mapping, when the file and the script disagree, or when a mapping changes anything but letter case. `ModelRecord.name` stays the unit's exact name (search and the other tabs read it); only `displayName` maps it.

2. **`catalog_id = (block_id, form)` — block_id is NOT unique.** The same amp `block_id` is catalogued as **both** a Combo and a Head (identical `block_name`). Dedup/keying by `block_id` alone silently drops rows — this is the "Amp Heads 48→3" regression. Key and dedup by `(block_id, form)`, and thread `form` (combo/head/half_stack) through `BlockArt` so art can tell them apart. `catalog.ts::isComboBid(model)` is the form lookup the strip call sites pass into `nodeTileArt`.

3. **Amp-id suffix parity — keep TS and Rust in lockstep.** A device amp id can carry merged `ConvRvb|CabIR|NoCab|Cab|IR` (+ a `NoFx` re-check) suffixes the catalog's bare id lacks. Matching is **check-first, then strip ONE suffix at a time, re-check** — never greedy, because reverb amps are catalogued _with_ the suffix. This lives in `deviceId.ts::resolveDeviceId` (TS — a zero-import leaf shared by `blockArt`, `cpu`, `catalog`) and `is_amp_model_id` (Rust, now in `src-tauri/src/probe_api/scene_jobs.rs`). A change to the suffix set or the NoFx bridge must edit **both** or amp detection silently diverges. Backend matches by EXACT FenderId, so a frontend-normalized id must never be passed into a backend op (see `notes/write-safety.md`).

4. **Null-bid blocks need a name fallback.** `resolveBlockArt(bid)` keys art by FenderId; the 7 Microphones have `block_id=null` (no `bid`) and fall through to a generic glyph unless resolved by NAME via `resolveBlockArtByName` (the `BY_NAME` index). Any new null-bid block needs the name fallback **and** a row in the `models-catalog.test.tsx` coverage test (it checks every catalog row, not spot samples).

## Import-cycle trap

`blockArt.ts` (and `blockArtCatalog/`) must **not** import `catalog.ts` or `cpu.ts` — that closed a module-init cycle (`blockArt→catalog→cpu→blockArt`) that crashes with a TDZ "cannot access before initialization". The shared id rule lives in the zero-import `deviceId.ts` leaf, which `blockArt`, `cpu` and `catalog` all import; cross-cutting form+art decisions resolve at the **view call site** (`toStripBlock`/`mkTile`, which may import both), never inside a core model module.

## Where things live

`src/models/catalog.ts` (ingest + taxonomy), `blockArt.ts` (id→art + `nodeTileArt` resolvers only — the rows themselves live elsewhere, see below), `cpu.ts` + `model-cpu.json` (per-block DSP cost), `tmp-model-guide.json` (the catalog data; the frontend bundles its `tmp-model-guide.slim.json` projection). Guard test: `src/__tests__/models-catalog.test.tsx`.

`TMP_CATALOG` rows live in `src/models/blockArtCatalog/`, one file per category (`bass.ts`, `cabBass.ts`, `cabGuitar.ts`, `combo.ts`, `delay.ts`, `drive.ts`, `dyn.ts`, `eq.ts`, `filter.ts`, `fw18.ts`, `halfstack.ts`, `mic.ts`, `mod.ts`, `pitch.ts`, `preamp.ts`, `reverb.ts`, `synth.ts`, `util.ts`, + `types.ts`/`index.ts`) — `blockArt.ts` only **imports** the assembled catalog and exposes the resolvers (`resolveBlockArt`, `resolveBlockArtByName`, `blockArtTile`, `nodeTileArt`); it does not hold row data itself. Per-model art treatments the icon + tone can't express (EVH channel colour, the Vibro-King baffle, the Glooper finish…) are an explicit `variant` token keyed by FenderId in `blockArtCatalog/variants.ts`, threaded through `BlockArt`'s `variant` prop — renderers never select art by matching the display caption. A new treatment = a new `ArtVariant` member (`ui/blockart/sharedIds.ts`) + a `variants.ts` entry + the oracle in `blockArtVariants.test.tsx`.

## Add a catalog row end-to-end

1. Pick the right `blockArtCatalog/<category>.ts` file for the block's category.
2. Add the row, keyed by `(block_id, form)` — a null-`bid` block (e.g. a Microphone) also needs a `BY_NAME` fallback entry (see invariant 4 above).
3. Extend the coverage oracle in `src/__tests__/models-catalog.test.tsx` (verify that file's actual name/oracle first — it may have shifted; adjust rather than assume).
4. If the row mirrors data in `tmp-model-guide.json`, flag the maintainer to mirror the change into their local generator (invariant 1) — don't rely on a future regen picking it up on its own.
5. Run `bun run test`.
