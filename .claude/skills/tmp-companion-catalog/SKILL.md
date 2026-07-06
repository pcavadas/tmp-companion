---
name: tmp-companion-catalog
description: "The data contract for the TMP Companion model catalog and block art. Use this skill whenever editing src/models/ — adding or changing a catalog row, touching block artwork (blockArt.ts / ui/blockart), the CPU-cost table, or the amp-id matching logic, or when reviewing a change to tmp-model-guide.json or a *.generated.ts file. Covers the four load-bearing invariants that break silently: the catalog is GENERATED (hand-edits are reverted + break the count oracle), block_id is non-unique across form so you must key by (block_id, form), the amp-id suffix-collapse must stay in lockstep between the TS and Rust sides, and null-bid blocks (the microphones) need a name-based art fallback. NOT for view/render code (→ tmp-companion-frontend) or what a block means in product terms (→ tmp-companion-data-model)."
---

# TMP Companion catalog

The Catalog tab and the signal-chain strip render from a shipped model catalog. This skill is the data contract for editing it without introducing a change that passes locally but is wrong.

**Orient first.** `CLAUDE.md`'s `models/` bullet is the authoritative map; this skill is the short checklist. When they disagree, `CLAUDE.md` wins — tell the user if you spot drift.

## The four invariants (each breaks silently)

1. **Generated output — never hand-edit.** `src/models/tmp-model-guide.json`, `blockColors.generated.ts`, and `cabCovering.generated.ts` are produced offline. A taxonomy/row edit to the JSON or a `*.generated.ts` file alone is **silently reverted on the next regen** _and_ breaks the count oracle in `models-catalog.test.tsx`. A JSON-only diff is a review smell: the generator source must change too — e.g. a bass amp's `form` lives in BOTH the JSON and the off-repo `expand_catalog.py` `BASS_FORM` map (the '66 Flip Top case). (Only `scripts/extract_cab_covering.py` — which regenerates `cabCovering.generated.ts` from the extracted client binary passed as `argv[1]` — ships in this repo; the main catalog pipeline (`pipeline.py` / `expand_catalog.py` / `colorcheck.py`) lives on the maintainer's machine, so a catalog-data change here is authored, reviewed against the oracle test, and regenerated upstream.)

2. **`catalog_id = (block_id, form)` — block_id is NOT unique.** The same amp `block_id` is catalogued as **both** a Combo and a Head (identical `block_name`). Dedup/keying by `block_id` alone silently drops rows — this is the "Amp Heads 48→3" regression. Key and dedup by `(block_id, form)`, and thread `form` (combo/head/half_stack) through `BlockArt` so art can tell them apart. `catalog.ts::isComboBid(model)` is the form lookup the strip call sites pass into `nodeTileArt`.

3. **Amp-id suffix parity — keep TS and Rust in lockstep.** A device amp id can carry merged `ConvRvb|CabIR|NoCab|Cab|IR` (+ a `NoFx` re-check) suffixes the catalog's bare id lacks. Matching is **check-first, then strip ONE suffix at a time, re-check** — never greedy, because reverb amps are catalogued _with_ the suffix. This lives in `blockArt.ts::resolveDeviceId` / `resolveCatalogId` (TS) and `is_amp_model_id` (Rust, `src-tauri/src/probe_api/scene_jobs.rs`). A change to the suffix set or the NoFx bridge must edit **both** or amp detection silently diverges. Backend matches by EXACT FenderId, so a frontend-normalized id must never be passed into a backend op (see `notes/write-safety.md`).

4. **Null-bid blocks need a name fallback.** `resolveBlockArt(bid)` keys art by FenderId; the 7 Microphones have `block_id=null` (no `bid`) and fall through to a generic glyph unless resolved by NAME via `resolveBlockArtByName` (the `BY_NAME` index). Any new null-bid block needs the name fallback **and** a row in the `models-catalog.test.tsx` coverage test (it checks every catalog row, not spot samples).

## Is my catalog edit safe? (decision block)

1. Editing a generated file (`tmp-model-guide.json`, `*.generated.ts`)? → STOP: edit the generator source too (or flag that the regen lives upstream), and update the `models-catalog.test.tsx` count oracles in the same change.
2. Keying/dedup'ing blocks? → key by `(block_id, form)`, never `block_id` alone.
3. Touching the suffix set or `NoFx` bridge? → change BOTH `blockArt.ts` (TS) and `is_amp_model_id` (Rust) in one commit.
4. Adding a block with `block_id=null`? → add the `resolveBlockArtByName` fallback + the coverage-test row.
5. Run `bun run test` — `models-catalog.test.tsx` checks EVERY row, not spot samples.

## Import-cycle trap

`blockArt.ts` must **not** import `catalog.ts` — that closes a module-init cycle (`blockArt→catalog→cpu→blockArt`) and crashes with a TDZ "cannot access before initialization". The safe direction is `catalog→blockArt`; cross-cutting form+art decisions resolve at the **view call site** (`toStripBlock`/`mkTile`, which may import both), never inside a core model module.

## Where things live

`src/models/catalog.ts` (ingest + taxonomy), `blockArt.ts` (id→art + `nodeTileArt`), `cpu.ts` + `model-cpu.json` (per-block DSP cost), `tmp-model-guide.json` (the catalog data). Guard test: `src/__tests__/models-catalog.test.tsx`.
