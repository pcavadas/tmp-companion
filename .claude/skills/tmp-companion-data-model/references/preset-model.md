# Preset model — signal-path topologies & block category counts

Companion to `SKILL.md` (product data model). This file holds the lookup-shaped detail behind four of its sections: the full 12-template topology table (`### Signal-path templates (12)`), the per-preset settings enumeration, the full text of the numbered firmware-enforced constraints, and the block category-count table (`## Block inventory`).

---

## Signal-path templates — the full topology table

**All twelve, enumerated** — do not collapse the three `Mix` rows when counting; that is how this list was previously mis-stated as 11 and elsewhere as 14. Topologies are read off the p.18 thumbnails: `■` = one element, `[a/b]` = a parallel group (upper lane / lower lane), `⇒` = lanes merge.

| #   | Template                         | Lanes       | Topology                                                                                     |
| --- | -------------------------------- | ----------- | -------------------------------------------------------------------------------------------- |
| 1   | `Instrument Series`              | 1           | `■–■–■–■–■` — pure series                                                                    |
| 2   | `Instrument Parallel 1`          | 1           | `■–■–[■/■]–■–■` — one split→mix section                                                      |
| 3   | `Instrument Parallel 2`          | 1           | `■–[■/■]–■–[■/■]–■` — **two** split→mix sections                                             |
| 4   | `Instrument Split`               | 1 in, 2 out | `■–` then two lanes of 3 that **never merge** (no mixer)                                     |
| 5   | `Mic/Line Series`                | 1           | identical shape to #1                                                                        |
| 6   | `Mic/Line Parallel 1`            | 1           | identical shape to #2                                                                        |
| 7   | `Mic/Line Split`                 | 1 in, 2 out | identical shape to #4                                                                        |
| 8   | `Instrument + Mic/Line Series`   | 2           | two rows of 5, fully independent, **never merge**                                            |
| 9   | `Instrument + Mic/Line Parallel` | 2           | two independent rows, each `■–■–[■/■]–■–■`, **never merge**                                  |
| 10  | `Instrument + Mic/Line Mix 1`    | 2 ⇒ 1       | two rows of 3 `⇒` one shared final element                                                   |
| 11  | `Instrument + Mic/Line Mix 2`    | 2 ⇒ 1       | **asymmetric** — instrument lane `■–[■/■]–■`, mic lane `■–■–■`, `⇒` one shared final element |
| 12  | `Instrument + Mic/Line Mix 3`    | 2 ⇒ 1       | **symmetric** — both lanes `■–[■/■]–■`, `⇒` one shared final element                         |

There is no `Mic/Line Parallel 2` and no 13th/14th template.

> The thumbnails are **schematic, not capacity diagrams** — the squares do not encode a block limit, and they do not distinguish a terminal (`INSTRUMENT`/`OUTPUT`) from a block. Real capacity is the CPU budget (`SKILL.md` constraint 5).

**Splitter vs Mixer glyph.** The prose implies two distinguishable symbols ("the symbol on the left … the symbol on the right"), but both the p.12 margin art and the p.18 Components diagram render the **same** 3-fader tile for each. Position in the chain is the only differentiator — relevant to `SignalChainView`, which must not rely on glyph identity.

---

## Preset identity — the `info` baseline field set (reference, moved from `SKILL.md`)

`preset_id` is verified unique across all factory presets. On every save the firmware re-serializes `info` to exactly these fields, dropping anything else: `author`, `created_at`, `displayName`, `preset_id`, `product_id`, `source_id`, `timestamp`, `version`. Injected keys therefore survive an import or restore but vanish on the first on-device edit+save (HW round-trip, fw 1.8.45). See `notes/write-safety.md` for why an in-place edit must preserve `preset_id`.

**Reaching the template picker:** `Preset Settings → Signal Path Type`, or touch `INSTRUMENT` / `MIC/LINE` at the far left of Preset View.

---

## Per-preset settings (reference, moved from `SKILL.md`)

- **Preset Volume**: 0–100% normalization
- **Input Impedance**: `Auto` (default — picks based on the first active amp/effect) | `22k` | `22k+330pF` | `330k` | `330k+330pF` | `1M` | `1M+330pF`. The 6 explicit options simulate the input impedance the modeled amp/effect would present.
- **Signal Path Type**: one of the 12 templates above
- **Output Assign**: 3×3 matrix `[Upper Path / Lower Path / USB 1-2] × [Headphones / Output 1 / Output 2]` — independently togglable
- **Preset MIDI**: up to 5 messages sent on preset load, each `(channel, PC#, CC#, CC value)`
- **Preset Spillover**: on/off — do delay/reverb tails continue across preset changes
- **AMP CTRL 1/2**: maps to the rear-panel AMP CTRL TRS jack (tip=AC1, ring=AC2) — wiring and insert-cable detail in `setup-recipes.md` §6
- **Tap tempo scope**: per-preset BPM **or** global tempo (Global Settings → Footswitch → Tap Tempo)

---

## Firmware-enforced constraints — detail (reference, moved from `SKILL.md`)

Detail behind the numbered constraints in `SKILL.md`. **The numbering there is cited externally (`workflows.md` cites 2, 5 and 8) — do not renumber it.**

1. **Convolution-reverb limit — 1 per preset.** The cap is on the shared **FFT convolution engine**, so membership is an `acdCategory` union, not a category name: standalone convolutions (`ACD_TMSpring63`/`65`, `Cathedral`, `HallOfDoom`, `EtherealHall`, and the room/plate/chamber models) **and** amps with baked-in convolution spring reverb (Deluxe / Princeton / Twin / Super Reverb, blackface and brownface — the `…CabIRConvRvb` ids). Total **19 members in 1.8.45, 16 in 1.7.75**. That is why those amps ship `NoFx` / `Normal` (reverb-free) variants — to free the one convolution slot.
2. **Cabinet limit — 2 cabinet-category blocks per preset.** Combo amps, half-stacks, standalone Cabinet blocks and IR blocks all share the same 2 slots, and a **dual-cab counts as 2 slots**. This is the firmware truth behind the Owner's-Manual "max 2 IR blocks" line.
3. **Glooper limit — 2 `ACD_Glooper` blocks per preset**, counted across both signal-chain rows.
4. **FX-loop coexistence — a rule, not a count.** The per-line-type slot-permission mask is: Guitar path → all slots allowed; Mic path → slots 0 and 1 denied; Split/Mix → all denied. On top of it a pairwise coexistence matrix makes the stereo `FxLoop3_4` mutually exclusive with the individual `FxLoop3` / `FxLoop4`. The "loops 1–2 before A/D, loops 3–4 after loop 2" ordering is **structural, not a runtime check**: Loops 1–2 are rear-panel fixed loops and are not add-block candidates at all (only `ACD_FxLoop3`/`4`/`3_4` have selectable profiles), and the Mic mask is what denies slots 0–1.
5. **Processor utilization — CPU budget, no fixed block count.** The **Add Block menu greys out** when adding a block would exceed the per-preset budget (76.5%, summed per-block `utilizationPercentage`; the numbers ship in `src/models/model-cpu.json`). Rejection strings: `Can't add/insert/replace node to guitar/mic group: over cpu budget`. This is the real "path is full" cap; the count limits stack on top of it.
6. **Loops 1 and 2 are fixed at the start of the Instrument path, BEFORE A/D.** They cannot be moved or placed in mic/line paths — they exist for analog pedals that need to interact with pickup impedance (fuzz, Rangemaster-style boost, vintage wah).
7. **Loops 3 + 4** pair as a single stereo loop **or** two mono loops — a per-preset choice (`Loop 3 mono`, `Loop 4 mono`, `Loop 3+4 stereo`). Placeable anywhere in the digital signal path after Loop 2. Both inputs cannot be set to off (`Both inputs cannot be set to off.`).
8. **IR blocks placement**: only after Loop 2, because they are digital. Count is governed by constraint 2.
9. **Scenes share blocks** — see the Scenes section of `SKILL.md`; scene slots are capped (`All scene slots full`).
10. **Splitter/Mixer are template-fixed** — see the signal-path template table above.
11. **Other capacity caps** surfaced as firmware rejections: `Cannot add new user preset in populated slot.`, saved block-presets (`BlockPresetLimitReached`), cloud/downloaded presets (`Downloaded Presets Limit Reached`).

---

## Block category counts (firmware 1.7)

Category counts for orientation (firmware 1.7):

| Category             | Models                   | Notes                                                                                                                               |
| -------------------- | ------------------------ | ----------------------------------------------------------------------------------------------------------------------------------- |
| Combo Amps           | 25+                      | Fender '57 Deluxe → Twin Reverb + JC Clean + Brit Breaker + UK 30 + Marksman                                                        |
| Half-Stack Amps      | 20+                      | British 45/Plexi/800/Jubilee, Hiway, FBE-100, EVH 5150 IIIS variants, Solo 100, Tangerine, Marksman CH2, Double Wreck, Petrol, Uber |
| Bass Amps            | 8                        | Bassman TV, Super Bassman, SWR Redhead, Rampage Blueline, '66 Flip Top, Rock-Bottom 400                                             |
| Amp Heads (cab-less) | every combo + half-stack | + Acoustasonic, Studio Preamp, Tube Preamp                                                                                          |
| Cabinets             | ~60                      | 1x10 through 8x10                                                                                                                   |
| Effects: Stompbox    | ~30                      | OD/distortion/fuzz                                                                                                                  |
| Effects: Modulation  | ~25                      | chorus/flanger/phaser/tremolo/rotary/vibe                                                                                           |
| Effects: Delay       | ~25                      | incl. Glooper, Arctic/Antarctic Sustainer, Stereo Doubler                                                                           |
| Effects: Reverb      | ~22                      | many **convolution** — see `SKILL.md` constraint 1                                                                                  |
| Effects: Dynamics    | ~11                      | compressors, gates, volume swell, slow attack                                                                                       |
| Effects: EQ          | ~10                      | 5/7/10-band graphic, 3/5-band parametric, LP/HP/notch filters                                                                       |
| Effects: Filter      | 5                        | 3 wahs + Filtron + Enigma envelope                                                                                                  |
| Effects: Pitch       | 10                       | Micro/Chromatic/Polygon/Polyvoice/Diatonic, Pedal Detune/Shifter, Virtual Capo, Granular Arpeggiator, Feedback Generator            |
| Effects: Synth       | 3                        | Cerberus + Aethon polysynths, Wavemorph                                                                                             |
