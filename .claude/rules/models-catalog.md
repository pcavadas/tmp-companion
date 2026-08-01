---
paths:
  - "src/models/**"
  # The catalog skill also governs the block-art engine, so the rule must load there too.
  - "src/ui/blockart/**"
---

# Catalog and block-art rules

Applies while editing `src/models/`. The full data contract is the `tmp-companion-catalog` skill; these are the invariants that break silently.

- **`blockArt.ts` must NOT import `catalog.ts`.** That closes the cycle `blockArt → catalog → cpu → blockArt`, a module-init TDZ "cannot access before initialization" crash. The SAFE direction is `catalog → blockArt`. Cross-cutting form+art decisions resolve at the VIEW call site (which may import both), never inside a core model module. **This is enforced** by `@typescript-eslint/no-restricted-imports` in `eslint.config.js` — if the rule fires, fix the direction, don't disable it.
- **The catalog is GENERATED.** A row-data change (form, category, glyph source) must ALSO edit the generator source, or the next pipeline regen silently reverts the JSON edit **and** breaks `models-catalog.test.tsx`'s count oracles.
- **Null-`bid` blocks need the name fallback.** `resolveBlockArt(bid)` keys art by FenderId; rows with `block_id=null` (the 7 Microphones) have no `bid` and fall through to the generic pedal glyph unless resolved by NAME via `resolveBlockArtByName`. Any new null-`bid` block needs the name fallback **plus** a row in the coverage test.
- **Key by `(block_id, form)`** — `block_id` is not unique across form.
- **Amp-id matching must be CHECK-FIRST then strip** (`CabIR`/`ConvRvb`). A discovered amp block can carry merged cab/IR/convolution suffixes the catalog's bare bid lacks, and the TS and Rust sides must stay in lockstep. [→ evidence](../../notes/gotchas.md#amp-id-matching-must-be-check-first-then-strip-cabirconvrvb)
- **`models/halfStack.ts` was DELETED** — keying the half-stack decision on catalog `form` always drew phantom cabs onto bare heads. The create decision is DEVICE-DRIVEN (it keys on `cabsimid` presence); catalog `form` is used only to SUPPRESS the stack for combo-form amps.
- **`isCombo` is a REQUIRED argument to `nodeTileArt`, with no default** — so a new caller cannot silently re-stack a combo whose modeled built-in speaker is also a `cabsimid`.
- **Block art is original SVG through the BlockArt engine — never an `<img>` photo tile** (Fender IP; the photo PNGs are not bundled).
