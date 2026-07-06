---
name: tmp-companion-data-model
description: "The product-facing data model for the Fender Tone Master Pro, from the official Owner's Manual + Model Guide. Use whenever you need what a preset, scene, block, signal-path template, cabinet/mic grid, footswitch or EXP assign, MIDI mapping, USB-audio/reamp route, or operating mode (My Presets / Favorites / Factory / Cloud / Songs / Setlists / DAW Mode / Looper) *means* in product terms — as opposed to the wire serialization (`tmp-companion-protocol`) or the catalog data contract (`tmp-companion-catalog`). Owns the 11 signal-path templates, scene-overlay semantics, the firmware-enforced constraints, per-preset settings, the screen inventory, and the MIDI chart. Grounds a Level/Copy/Songs feature so it is implemented semantically correctly (e.g. all scenes share one block list, reamp bypasses the analog Loops 1–2, presetLevel is a global multiplier while a scene is leveled on its amp's outputLevel)."
---

# TMP product-facing data model

Source: Fender's v1.7 Owner's Manual (49 pp) + Model Guide (127 pp) — the public product documentation. This skill captures what the device _models_: what a preset/scene/block _is_ in product terms, so the companion code named in it (`audiograph.rs` `FenderId`/`scenes[]`/`ftsw`, `leveller.rs` `presetLevel` vs per-scene `outputLevel`, `audio.rs` USB routing, `models/` catalog) is implemented with the right meaning.

## Which source wins? (decision block)

- **Architecture / where code lives** → `CLAUDE.md` (it wins over this skill on any disagreement).
- **Wire bytes / serialization** → `tmp-companion-protocol` + `notes/protocol.md`.
- **Catalog id→name/art/category data** → `tmp-companion-catalog` (and `product_profile.json` outranks the Model Guide on on-device availability; the Model Guide wins on official names/attributions).
- **Write persistence rules** → `notes/write-safety.md`.
- **Product meaning of a concept** → this skill; verified against real HW where noted (fw 1.7.75 / 1.8.45).

## Preset object

A preset = signal-path template + blocks + per-preset settings + footswitch assigns + EXP assigns + scenes.

> **Preset identity = `presetJson.info.preset_id`** (a UUID, unique per preset; verified unique across all factory presets). It is the stable join key for host-side per-preset metadata — NOT the user-editable `displayName`, NOT the positional slot. The on-device DB stores `presetJson` as PLAINTEXT JSON (the XOR+LZ4 encoding is only the exported `.preset` file). The firmware re-serializes `info` to a FIXED baseline field set (`author, created_at, displayName, preset_id, product_id, source_id, timestamp, version`) on every save — so extra keys injected into `info` survive an import/restore but are DROPPED on the first on-device edit+save (HW round-trip, fw 1.8.45). Durable host metadata therefore lives in a companion sidecar keyed by `preset_id`, not in the preset. (See `notes/write-safety.md` for why an in-place edit must preserve `preset_id`.)

| Store                       | Capacity | Addressing                                                                               |
| --------------------------- | -------- | ---------------------------------------------------------------------------------------- |
| User presets ("My Presets") | 504      | 4 MIDI banks: banks 0–2 = 128 each, bank 3 = 120 — Bank Select CC0=0..3 + PC 1..128      |
| Songs                       | 200      | each = ordered preset bank for one song, sections labeled (intro/verse/chorus/solo/…)    |
| Setlists                    | 50       | each = up to 99 Songs in sequence                                                        |
| Cloud presets               | 100      | not numbered, newest first, downloaded via the TMP Control desktop app                   |
| User Block Presets          | 500      | **separate persistence store** — per-block defaults saved by the user (Add Block menu ▾) |

### Signal-path templates (11)

Choose ONE template per preset, then populate. Splitter and Mixer are template-fixed — predetermined positions in parallel templates, not user-addable/removable. The set: `Instrument Series` · `Instrument Parallel 1/2` · `Instrument + Mic/Line Series` · `Instrument + Mic/Line Mix 1/2/3` · `Instrument + Mic/Line Parallel` · `Mic/Line Series` · `Mic/Line Parallel 1` · `Instrument Split` · `Mic/Line Split`.

### Block types

Combo Amp / Half-Stack / Bass Amp (each = model + Cabinet sub-block), Amp Head (cab-less; manual cab pairing or none), standalone Cabinet (IR collection + mic config), Effect (~150 models in 9 categories — the `tmp-companion-catalog` skill owns the shipped id→name/category data), FX Loop 1–4 (physical loop placement markers), Splitter/Mixer (template-fixed), Impulse Response (shares the 2 cabinet-category slots, placement only after Loop 2).

### Per-preset settings

Preset Volume (0–100%) · Input Impedance (`Auto` + 6 explicit RC options) · Signal Path Type · Output Assign (3×3 matrix `[Upper/Lower/USB 1-2] × [Headphones/Out 1/Out 2]`) · Preset MIDI (up to 5 messages on load) · Preset Spillover (tails across preset changes) · AMP CTRL 1/2 (rear TRS tip/ring latching closures) · Tap-tempo scope (per-preset BPM or global).

### Cabinet sub-model

Per amp block or standalone Cabinet block: 1–2 cabs ("+ Add Cab"), 1–2 mics per cab. Per mic: model (7 options: C414, M23, MD421, R121, RE20, SM7B, SM57) · position (**32-slot grid** = 4 vertical positions × 8 distances, each cell a distinct IR) · on/off-axis · low-cut + high-cut. Dual-mic/dual-cab adds Blend + Pan 1/2. **External Cabinet** bypasses the internal IR and exposes the **Speaker Impedance Curve (SIC)** parameter. The on-disk IR naming (`{Cabinet}_{Speaker}_{mic}_{position}_{axis}_{distance}.wav`) indexes exactly this grid.

## Scenes

Up to **9 per preset** (8 footswitch-recallable + base preset).

Invariants (firmware-enforced): all scenes share the **same signal-path template, same block list, same block order**; adding/replacing a block in any scene lands in _all_ scenes; changing block order or template affects all scenes.

Per-scene differences: bypass state of each block; per-block parameter values gated by the per-block **Scene Edit flag** (`ENABLED` default: changes apply only to the active scene; `DISABLED`: shared across all scenes); scene-specific Amp Control / MIDI PC / MIDI CC messages.

**Scene Change Behavior** (Global Settings → Footswitch): `Maintain Changes` (default — reload preserves unsaved edits) vs `Discard Changes`. The serialization is a sparse diff (`ftswStates` + scene-keyed override maps) — see `tmp-companion-protocol` / `notes/protocol.md`.

## Footswitch Assign (Effects FS mode, per preset)

8 of 10 physical footswitches are assignable (2 fixed: FS Mode toggle, Tap/Tuner). Each carries up to **5 functions**: `ON/OFF` (toggle blocks; multi-block = `MULTI`), `Parameter Change` (toggle one param between two values), `Scene`, `MIDI CC`, `MIDI PC`, `Amp Control`. Per-function fields: assigned block(s), Active/Inactive colors, latching/momentary, custom label, **Switch Link group ID** (mutex group).

> **Footswitch-gated parameters default to OFF.** A block parameter can store `0` in `dspUnitParameters` and only reach its real value via a footswitch Parameter Change (`func:"param"`, `valueA` = engaged value). With the footswitch disengaged (`isActive=false`, the default) the param stays 0 — a "silent" effect in the preset JSON may be _gated off_, not absent. Don't read a block's presence as "audibly doing something."

## EXP Assign (per preset)

Five sources (`Toe Switch`, `EXP 1`, `EXP 2`, virtual `MIDI EXP 3/4` via CC 3/4), each with up to **5 parameter targets**: block+param, heel/toe values, 5-option taper, Switchless Bypass (off/heel/toe, 300 ms hysteresis), optional external MIDI CC. **EXP Live Mode** reads the live pedal position at preset load (the "global volume pedal" pattern).

## MIDI implementation

Full table: **`references/midi-cc-map.md`**. Headlines: 504 presets via Bank Select CC0 (0–3) + PC 1..128 (1..120 in bank 3); receive channel 1–16/Omni; MIDI Out jack `Out`/`Thru`/`Merge`; PC/CC/Clock independently enableable per jack/USB. **Gotcha:** receive-clock ON disables the tap-tempo footswitch and overrides per-preset saved tempos. Load-bearing CCs: 1/2 = EXP 1/2 · 3/4 = MIDI EXP 3/4 · 7 = Master Volume · 20 = FS Mode · 21–29 = FS 1–8 toggles · 30 = bulk FS enable/disable · 64 = Tap · 65 = Toe · 66/67 = Amp Control 1/2 · 68 = Tuner · 69/70 = Next/Prev Song · 103–110 = Looper transport.

## USB audio routing

TMP enumerates as a **4-in / 4-out** USB 2.0 audio interface (44.1–96 kHz DAW-selectable, 32-bit engine).

**Standard mode:** USB Out 1/2 = processed stereo; Out 3 = dry instrument (pre-DSP, pre-Loops-1/2); Out 4 = dry mic/line. USB In 1/2 = DAW stereo → assignable to outputs; In 3/4 disabled. PRE/POST toggle in the Output Mixer controls the USB 3/4 sends.

> **USB Out 3 (dry instrument) has no limiter** — it clips at 0 dBFS for hot pickups played hard (live-confirmed). From the Mac's perspective it's input channel index 2; it feeds the Tier-2 calibration path.

**Reamp mode** (per-preset, manual toggle; resets OFF on power cycle): USB In 3 → instrument channel's **first signal-path block** (mutes the rear instrument input); In 4 → mic/line channel's first block. **Loops 1 and 2 are bypassed** (analog, pre-A/D — a USB-injected track can't reach them); Loops 3/4 stay active.

> **The reamp inject is NOT AGC'd** — the injected track's amplitude directly drives the block's nonlinearity; apparent flattening is the amp model's own compression (live-confirmed via an amplitude sweep: ~1.65 LU out per 6 dB in on a clean preset, linearizing at low drive). A hotter instrument genuinely drives the chain harder — the foundation of instrument-aware leveling.

## Firmware-enforced constraints (summary — full list: `references/constraints.md`)

The ones that shape companion features: **1 convolution-reverb per preset** (the FFT engine cap; membership includes the `…CabIRConvRvb` amps — why `NoFx` variants exist); **2 cabinet-category slots** (combos/half-stacks/cabs/IRs share them; dual-cab = 2); **CPU budget 76.5%** (the real "path is full" cap — `src/models/model-cpu.json`); **scenes share one block list**; **Splitter/Mixer are template-fixed**. The companion mirrors the caps in `src-tauri/src/blockcaps.rs` / `src/views/copy/validateBlockEdit.ts`.

## Operating modes & screens (full tables: `references/modes-and-screens.md`)

Six navigation modes (My Presets 504 / Favorites / Factory / Cloud 100 / Songs 200 / Setlists 50) + the modal DAW Mode and Looper. `tabEnum`: `UserPresets=1 … Songs=5, Setlists=6`; the cursive "F" badge is **Factory** (`tabEnum=4`), not Favorites. The screen inventory (Preset View, Gig View, Block Edit, Cabinet Settings, Mixer, …) and the block-inventory orientation counts live in the reference.

## Why this matters for the companion

- **Copy** (`src/views/copy/copyModel.ts` + `audiograph.rs`): because **all scenes share one block list**, a Copy insert/remove must land in every scene, and a block lives in three keyed places (see `notes/write-safety.md` + `notes/block-copy.md`).
- **Leveling** (`leveller.rs` / `audio.rs`): `presetLevel` is a **global multiplier** over all scenes → level the base scene first; each footswitch scene is leveled on its **active amp's `outputLevel`**. Reamp routes the DAW track into the chain's first block and bypasses the analog Loops 1–2, and the inject is not AGC'd — the model above is why.
- **Signal chain + Catalog** (`SignalChainView` / `models/`): the 11 templates + block types + the cabinet sub-model are what the strip renders; `tmp-companion-catalog` owns the id→art/name mapping.

## Sources

Fender Tone Master Pro Interactive Owner's Manual (fw v1.7, 49 pp) + Model Guide (fw v1.7, 127 pp). Re-fetch from Fender's product page when firmware revs. Firmware 1.8 ships 31 new models, so the model inventory is one generation behind — the structural model (templates, scenes, footswitch/EXP, constraints) is stable across 1.7→1.8.
