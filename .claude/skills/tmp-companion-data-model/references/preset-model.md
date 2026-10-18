# Preset model — signal-path topologies, settings, scenes & constraints

Companion to `SKILL.md` (product data model). This file holds the lookup-shaped detail behind its sections: the full 12-template topology table (`### Signal-path templates (12)`), the per-preset settings enumeration, how scenes are stored, and the full text of the numbered firmware-enforced constraints.

---

## Signal-path templates — the full topology table

**All twelve, enumerated** — count the three `Mix` rows separately. `audioGraph.template` carries the firmware id (firmware data, fw 1.8.58). Topologies are read off the p.18 thumbnails: `■` = one element, `[a/b]` = a parallel group (upper lane / lower lane), `⇒` = lanes merge.

| #   | Template                         | Firmware id      | Lanes       | Topology                                                                                     |
| --- | -------------------------------- | ---------------- | ----------- | -------------------------------------------------------------------------------------------- |
| 1   | `Instrument Series`              | `gtrSeries`      | 1           | `■–■–■–■–■` — pure series                                                                    |
| 2   | `Instrument Parallel 1`          | `gtrParallel1`   | 1           | `■–■–[■/■]–■–■` — one split→mix section                                                      |
| 3   | `Instrument Parallel 2`          | `gtrParallel2`   | 1           | `■–[■/■]–■–[■/■]–■` — **two** split→mix sections                                             |
| 4   | `Instrument Split`               | `gtrSplit`       | 1 in, 2 out | `■–` then two lanes of 3 that **never merge** (no mixer)                                     |
| 5   | `Mic/Line Series`                | `micSeries`      | 1           | identical shape to #1                                                                        |
| 6   | `Mic/Line Parallel 1`            | `micParallel1`   | 1           | identical shape to #2                                                                        |
| 7   | `Mic/Line Split`                 | `micSplit`       | 1 in, 2 out | identical shape to #4                                                                        |
| 8   | `Instrument + Mic/Line Series`   | `gtrMicSeries`   | 2           | two rows of 5, fully independent, **never merge**                                            |
| 9   | `Instrument + Mic/Line Parallel` | `gtrMicParallel` | 2           | two independent rows, each `■–■–[■/■]–■–■`, **never merge**                                  |
| 10  | `Instrument + Mic/Line Mix 1`    | `gtrMicMix`      | 2 ⇒ 1       | two rows of 3 `⇒` one shared final element                                                   |
| 11  | `Instrument + Mic/Line Mix 2`    | `gtrMicMix2`     | 2 ⇒ 1       | **asymmetric** — instrument lane `■–[■/■]–■`, mic lane `■–■–■`, `⇒` one shared final element |
| 12  | `Instrument + Mic/Line Mix 3`    | `gtrMicMix3`     | 2 ⇒ 1       | **symmetric** — both lanes `■–[■/■]–■`, `⇒` one shared final element                         |

There is no `Mic/Line Parallel 2` and no 13th/14th template.

> The thumbnails are **schematic, not capacity diagrams** — the squares do not encode a block limit, and they do not distinguish a terminal (`INSTRUMENT`/`OUTPUT`) from a block. Real capacity is the CPU budget (`SKILL.md` constraint 5).

**Splitter vs Mixer glyph.** The prose implies two distinguishable symbols ("the symbol on the left … the symbol on the right"), but both the p.12 margin art and the p.18 Components diagram render the **same** 3-fader tile for each. Position in the chain is the only differentiator — relevant to `SignalChainView`, which must not rely on glyph identity.

---

## Preset identity — the `info` field set

On every save the firmware re-serializes `info` to exactly these fields, dropping anything else: `author`, `created_at`, `displayName`, `preset_id`, `product_id`, `source_id`, `timestamp`, `version`. Injected keys therefore survive an import or restore but vanish on the next save. Every save also mints a fresh `preset_id`, a save to the same slot included; import, move and restore keep the body's id, so ids can repeat across slots. The 256 factory ids are distinct (static RE + HW, fw 1.8.58).

**Reaching the template picker:** `Preset Settings → Signal Path Type`, or touch `INSTRUMENT` / `MIC/LINE` at the far left of Preset View.

---

## Per-preset settings

- **Preset Volume**: 0–100% normalization
- **Input Impedance**: `Auto` (default — picks based on the first active amp/effect) | `22k` | `22k+330pF` | `330k` | `330k+330pF` | `1M` | `1M+330pF`. The 6 explicit options simulate the input impedance the modeled amp/effect would present.
- **Signal Path Type**: one of the 12 templates above
- **Output Assign**: 3×3 matrix `[Upper Path / Lower Path / USB 1-2] × [Headphones / Output 1 / Output 2]` — independently togglable. Stored as the preset's `outputMixerSettings` sources `instrumentOutput`, `comboOutput` and `USB12Input`; every load copies them over the global mixer's `instrumentActive`, `micActive` and `usb12Active` (HW), so the USB 1/2 routing can change with each preset loaded. That `Upper Path` / `Lower Path` are the instrument and mic/line channels is inferred from those key names.
- **Preset MIDI**: up to 5 messages sent on preset load, each `(channel, PC#, CC#, CC value)`
- **Preset Spillover**: on/off (`audioGraph.spillover`) — do delay/reverb tails continue across preset changes. The OUTGOING preset's flag decides: on, its DSP keeps sounding with zero input until the next preset load deletes it; off, its output is cut within ~0.1 ms (static RE, fw 1.8.58). The setting does not apply to scene recalls (p.17).
- **AMP CTRL 1/2**: maps to the rear-panel AMP CTRL TRS jack (tip=AC1, ring=AC2) — wiring and insert-cable detail in `setup-recipes.md` §6
- **Tap tempo scope**: per-preset BPM **or** global tempo (Global Settings → Footswitch → Tap Tempo)

---

## Scene storage — overlays and the Scene Edit flag

Each `scenes[N]` carries per-node overlays keyed by group and `nodeId`, plus `ftswStates` (static RE, fw 1.8.58):

- **The overlay's shape IS the Scene Edit flag.** An overlay with more than one key is a full snapshot (Scene Edit ENABLED: the block's knobs are isolated from base in that scene); a `{bypass}`-only overlay is Scene Edit DISABLED (knobs shared with base, only on/off scene-tracked); no overlay renders base. Base blocks carry no flag.
- **Where a `changeParameter` lands in scene N:** in the overlay when the param is `bypass` on an existing entry, or the entry has more than one key; otherwise on base, so every scene sharing that block gets it.
- **Recall:** a `{bypass}`-only overlay merges that one key onto base; any other overlay replaces the node's whole `dspUnitParameters` object.
- **Defaults (HW):** a scene added after the blocks exist leaves them unflagged in it; a block added after scenes exist is flagged in every existing scene, snapshotting its add-time values — which is why a scene can show a block's defaults while base was tuned later.
- **`ftswStates`:** every recall writes the target scene's `ftswStates` into the footswitches' `isActive` flags. That sets each switch's LED and what its next press does; what is heard at the recall comes only from the node overlays. A scene footswitch's `isActive` toggles on each press, and a press while it is set loads base (scene 8).

---

## Firmware-enforced constraints — detail

Detail behind the numbered constraints in `SKILL.md`. **The numbering there is cited externally (`workflows.md` cites 2, 5 and 8) — do not renumber it.**

1. **Convolution-reverb limit — 1 per preset.** The cap is on the shared **FFT convolution engine**, so membership is an `acdCategory` union, not a category name: standalone convolutions (`ACD_TMSpring63`/`65`, `Cathedral`, `HallOfDoom`, `EtherealHall`, and the room/plate/chamber models) **and** amps with baked-in convolution spring reverb (Deluxe / Princeton / Twin / Super Reverb, blackface and brownface — the `…CabIRConvRvb` ids). The shipped set (`block-classification.json`) has **19** members. That is why those amps ship `NoFx` / `Normal` (reverb-free) variants — to free the one convolution slot.
2. **Cabinet limit — 2 cabinet-category blocks per preset.** The control app counts combos, half-stacks, bass amps, Cabinet blocks and IR blocks; heads and `ACD_ExternalCab` don't count, and a block with its second cab or second mic enabled counts 2 (static RE, fw 1.8.58). This is the firmware truth behind the Owner's-Manual "max 2 IR blocks" line. `blockcaps.rs` still counts `ACD_ExternalCab` and does not weight an amp with a second mic as 2: its sets were extracted from fw 1.8.45.
3. **Glooper limit — 2 `ACD_Glooper` blocks per preset**, counted across both signal-chain rows.
4. **FX-loop coexistence — a rule, not a count.** The per-line-type slot-permission mask is: Guitar path → all slots allowed; Mic path → slots 0 and 1 denied; Split/Mix → all denied. On top of it a pairwise coexistence matrix makes the stereo `FxLoop3_4` mutually exclusive with the individual `FxLoop3` / `FxLoop4`. The "loops 1–2 before A/D, loops 3–4 after loop 2" ordering is **structural, not a runtime check**: Loops 1–2 are rear-panel fixed loops and are not add-block candidates at all (only `ACD_FxLoop3`/`4`/`3_4` have selectable profiles), and the Mic mask is what denies slots 0–1.
5. **Processor utilization — CPU budget, no fixed block count.** The **Add Block menu greys out** when adding a block would exceed the per-preset budget (76.5%, summed per-block `utilizationPercentage`; the numbers ship in `src/models/model-cpu.json`). Rejection strings: `Can't add/insert/replace node to guitar/mic group: over cpu budget`. This is the real "path is full" cap; the count limits stack on top of it.
6. **Loops 1 and 2 are fixed at the start of the Instrument path, BEFORE A/D.** They cannot be moved or placed in mic/line paths — they exist for analog pedals that need to interact with pickup impedance (fuzz, Rangemaster-style boost, vintage wah).
7. **Loops 3 + 4** pair as a single stereo loop **or** two mono loops — a per-preset choice (`Loop 3 mono`, `Loop 4 mono`, `Loop 3+4 stereo`). Placeable anywhere in the digital signal path after Loop 2. Both inputs cannot be set to off (`Both inputs cannot be set to off.`).
8. **IR blocks placement**: only after Loop 2, because they are digital. Count is governed by constraint 2.
9. **Scenes share blocks** — see the Scenes section of `SKILL.md`; scene slots are capped (`All scene slots full`).
10. **Splitter/Mixer are template-fixed** — see the signal-path template table above.
11. **Other capacity caps** surfaced as firmware rejections: `Cannot add new user preset in populated slot.`, saved block-presets (`BlockPresetLimitReached`), cloud/downloaded presets (`Downloaded Presets Limit Reached`).
