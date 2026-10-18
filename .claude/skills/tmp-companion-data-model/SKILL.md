---
name: tmp-companion-data-model
description: "Product-facing data model for the Fender Tone Master Pro, from the official Owner's Manual and Model Guide. Use for what a preset, scene, block, signal-path template, cabinet/mic grid, footswitch or EXP assign, MIDI mapping, USB-audio or reamp route, or operating mode (My Presets, Favorites, Factory, Cloud, Songs, Setlists, DAW Mode, Looper) means in product terms — rather than the wire serialization (`tmp-companion-protocol`) or the catalog contract (`tmp-companion-catalog`). Also covers device behaviour and operating questions: switch link, song tempo, backup and firmware update, 4CM wiring, feeding a stage amp and the PA together, Global Settings defaults, and the questions the manual leaves open."
---

# TMP product-facing data model

The domain dictionary for TMP's ubiquitous language — what a preset, scene or block _is_ in product terms — from Fender's Owner's Manual + Model Guide, so the companion code carries the right meaning.
`AGENTS.md` is the agent guide and wins any disagreement here; device behaviour is in `notes/device/` and the wire encoding is the `tmp-companion-protocol` skill.

## Preset object

A preset = signal-path template + blocks + per-preset settings + footswitch assigns + EXP assigns + scenes.

> **`presetJson.info.preset_id` is a UUID, not a stable identity.**
> Every `saveCurrentPreset` re-mints it, a save to the same slot included; import and restore keep whatever id the body carries, and nothing checks uniqueness, so two slots can share one (static RE + HW, fw 1.8.58).
> Use it only as a per-slot "has this body changed since I read it" check, which is what `bulk_replace.rs`, `backup_read.rs` and `commands/presets.rs` do.
> On every save the firmware re-serializes `info` to 8 fixed keys, so keys injected into `info` survive an import but vanish on the next save — host-side metadata belongs in a **sidecar** keyed by slot.
> The on-device DB stores `presetJson` as **plaintext JSON**; only the exported `.preset` file is encoded, and that is **XOR-only** — go through the shared seams `backup::xor_jld` and `library::decode_preset_bytes`, never a second codec (LZ4 wraps the bytes solely inside `importPresetRequest.presetJson`).
> Field list: `references/preset-model.md`; identity and in-place-edit rules: [`notes/device/presets.md`](../../../notes/device/presets.md#preset-identity).

Capacities: **504** user presets (4 MIDI banks — 128/128/128/120) · **200** Songs (each an ordered preset bank with labelled sections) · **50** Setlists (≤99 Songs each) · **100** Cloud presets · **500** User Block Presets, a **separate persistence store** of per-block user defaults.

### Signal-path templates (12)

Choose ONE template per preset, then populate.
Splitter and Mixer are **template-fixed** — fixed positions in the parallel templates, not independently addable or removable.
Changing the template after the path is populated **repopulates** the existing blocks into the new shape (p.18).

There are **twelve** — count the three `Mix` rows separately.
Topology table: `references/preset-model.md`; block types: `references/workflows.md` §2.

## Per-preset settings

Preset Volume, Input Impedance (`Auto` + 6 explicit values), Signal Path Type, the 3×3 Output Assign matrix, up to 5 Preset MIDI messages, Preset Spillover, AMP CTRL 1/2, and tap-tempo scope.
Values and defaults: `references/preset-model.md`.

## Scenes

Up to **9 per preset** (8 footswitch-recallable + base preset).

Invariants (firmware-enforced):

- All scenes share the **same signal-path template, same block list, same block order**
- Adding a block to any scene loads it (enabled) in _all_ scenes
- Replacing a block in any scene replaces it in all scenes
- Changing block order or signal-path template affects all scenes

What a scene _can_ differ in: each block's bypass state; per-block parameter values, gated by the per-block **Scene Edit flag** (`ENABLED`, the default, applies a change only to the active scene; `DISABLED` shares it across all scenes); and its own Amp Control / MIDI PC / MIDI CC messages.

Whether a recalled scene keeps unsaved edits is the global **Scene Change Behavior** setting, default `MAINTAIN CHANGES` (`references/global-settings.md`); loading another preset always discards them.
The serialization is a sparse diff — `ftswStates` plus scene-keyed node overlays, whose shape IS the Scene Edit flag (`references/preset-model.md`).

## Footswitch Assign (Effects FS mode, per preset)

8 of 10 physical footswitches are assignable (FS Mode toggle and Tap/Tuner are fixed), each carrying up to **5 functions simultaneously**.
Tables: `references/workflows.md` §4.

**Field scope — switch-level vs function-level.**
`Type` and `Block` are **per function**; `Color (Active/Inactive)`, `Switch` (latching/momentary) and `Custom Label` are **switch-level** in the UI, one value shared by every function on that footswitch.
The firmware stores all of them on each function entry (static RE, fw 1.8.58), so a switch-level field must carry the same value on every function of the switch.

**Switch Link** is a mutual-exclusion group of up to **8 footswitches**, stored as `linkGroup` on each function entry (0 = none).
Pressing an inactive entry with a non-zero `linkGroup` first turns off, and executes, every active entry of the same group on the other switches (static RE, fw 1.8.58; that the UI's Switch Link row writes `linkGroup` is inferred).

**Footswitch-gated parameters default to OFF** — a block parameter can store `0` and only reach its real value via a `Parameter Change` function, so a "silent" effect in the preset JSON may be _gated off_, not absent.

## EXP Assign (per preset)

Five expression sources (Toe Switch, EXP 1, EXP 2, MIDI EXP 3, MIDI EXP 4), up to 5 parameter targets each.
Taper, Switchless Bypass, EXP Live Mode: `references/workflows.md` §5.

## Block inventory

Don't duplicate the catalog here — it stales on every firmware update.
**Source precedence is scoped per fact:** the Model Guide wins on official names and appearance; the firmware's `product_profile.json` outranks it on on-device availability and menu category.
`tmp-companion-catalog` owns the shipped `tmp-model-guide.json`.

## MIDI implementation

All of it — CC chart, bank/PC addressing, MIDI Out modes, receive channel and filtering, clock, and six gotchas (receive-clock disables the tap footswitch; CC 25 is skipped; CC 30 is a bulk control) — is in **`references/midi-cc-map.md`**.

## USB audio routing

A **4-in / 4-out** USB 2.0 audio interface, 44.1 / 48 / 88.2 / 96 kHz DAW-selectable.
Channel maps for standard vs reamp mode, the reamp/AGC model and the PRE/POST fader detail: `references/setup-recipes.md` §4.

> **The USB clock rate is not the internal rate.**
> The DSP and its I²S run at a fixed **44.1 kHz**; the USB side converts the host rate to and from it with a synchronous SRC, pitch-exact and slip-free at all four rates, and the global `sampleRate` setting is only stored and echoed (static RE + HW, fw 1.8.58).
> So re-amp capture above ~22 kHz is anti-alias skirt, not preset tone — load-bearing for spectrum / EQ-match / Doctor-PSD work.
> The **host Core Audio rate stays 48 kHz**: what `audio.rs` requires, not a bug to "fix" (`.claude/rules/danger.md`, "48 kHz stimulus required").

## Firmware-enforced constraints

The caps below are **client-side validation only**: enforced by the firmware's **control app**, while the device's audio engine does **not** reject an over-cap edit — it enforces only the CPU budget.
Any new apply path must call the cap check itself, because nothing downstream will.
The companion mirrors them in `blockcaps.rs` / `validateBlockEdit.ts`; where its membership sets (`src/models/block-classification.json`) differ from the firmware's is stated in [`notes/device/presets.md`](../../../notes/device/presets.md).
**Full text in `references/preset-model.md`; the numbering below is cited externally, so never renumber it.**

1. **Convolution reverb — 1 per preset.**
   The cap is on the shared FFT convolution engine, so it also catches amps with baked-in spring reverb (`…CabIRConvRvb`), which is why those ship `NoFx`/`Normal` variants.
2. **Cabinets — 2 per preset.**
   Combo amps, half-stacks, bass amps, Cabinet blocks and IR blocks share the 2 slots (heads and External Cabinet don't count); a block with its second cab or second mic on counts 2.
3. **Glooper — 2 per preset**, across both rows.
4. **FX-loop coexistence — a rule, not a count.**
   A slot-permission mask per line type plus a pairwise exclusion matrix.
5. **Processor utilization — a CPU budget, not a block count** (76.5% per preset).
   This is the real "path is full" cap; the count limits stack on top of it.
6. **Loops 1 and 2 are fixed at the start of the Instrument path, BEFORE A/D** — unmovable, and absent from mic/line paths.
7. **Loops 3 + 4** are one stereo loop or two mono loops, per preset, anywhere after Loop 2.
8. **IR blocks** may only sit after Loop 2.
   Count is governed by constraint 2.
9. **Scenes share blocks** (see Scenes above); scene slots are capped.
10. **Splitter/Mixer are template-fixed** (see Signal-path templates above).
11. **Other capacity caps** surface as rejection strings: populated slot, block-preset limit, downloaded-preset limit.

## Operating modes

Six navigation modes via the left-side touchscreen icons: My Presets, Favorites, Factory Presets, Cloud Presets, Songs, Setlists.
Capacity/behaviour table: `references/workflows.md` §1; DAW Mode and Looper entry gestures: `references/device-operation.md` §5 and §3; the screen/modal index is that file's tail.

Each mode has a `tabEnum` on the wire (`tmp-companion-protocol`).
One product consequence: **the cursive "F" top-bar badge is Factory (`tabEnum=4`), not Favorites** — the preset came from the factory tab, so the brand mark shows instead of a numeric slot.
My-Presets selections (`tabEnum=1`) render the slot number.

## Why this matters for the companion

- **Copy**: because **all scenes share one block list**, an insert or remove must land in every scene, and the block lives in three keyed places ([three keyed places](../../../notes/device/presets.md#three-keyed-places)).
  Copy applies live structural edits (`commands/copy_apply.rs` → `session::{replace_node,insert_node,remove_node}`) and walks with `audiograph::for_each_node{,_mut}` + `node_id` — never a private traversal.
- **Leveling**: `presetLevel` is a **global multiplier** over all scenes, so the base scene is levelled first; each footswitch scene is levelled on its **active amp's `outputLevel`**.
  Reamp bypasses the analog Loops 1–2.
  Mechanism: [`notes/leveling.md`](../../../notes/leveling.md).
- **Signal chain + Catalog**: the 12 templates, block types and cabinet sub-model are what the strip renders; `tmp-companion-catalog` owns the id→art/name mapping.

## References

- `preset-model.md` — 12-template topology table, per-preset settings, scene overlay storage, the numbered constraints in full.
- `midi-cc-map.md` — full CC chart, bank/PC addressing, Out modes, clock, six gotchas.
- `setup-recipes.md` — rear-panel jacks as silkscreened, output levels, USB channel maps, the reamp/AGC model, the four documented rigs (incl. 4CM).
- `global-settings.md` — all seven Global Settings tabs and **every default**, Global EQ bands, Output Mixer, tuner, override precedence.
- `workflows.md` — editing on the unit: presets, blocks, the **cabinet sub-model** (mic models, the 32-slot position grid, axis/filters, dual-cab/dual-mic Blend+Pan, External Cabinet/SIC — §3), footswitch and EXP assignments, the operating-modes capacity table.
- `device-operation.md` — playing and maintaining the unit: songs (incl. song BPM), setlists, Looper, tap tempo and tuner, DAW Mode, backup and firmware update, power-up, the screen index.
- `manual-errata.md` — where the manual contradicts itself or its artwork.
  Device questions still open: [`notes/open-questions.md`](../../../notes/open-questions.md) — **read it before asserting a device behaviour this skill does not state.**

Sources: the Interactive Owner's Manual (structural facts verified against **firmware v1.8**, rev. J) and the Model Guide (v1.7).
The inventory stays v1.7-pinned — 1.8 ships 31 models the guide doesn't cover.
The structural model is stable across 1.7→1.8.
