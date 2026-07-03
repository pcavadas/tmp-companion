---
name: tmp-companion-data-model
description: "The product-facing data model for the Fender Tone Master Pro, from the official Owner's Manual + Model Guide. Use whenever you need what a preset, scene, block, signal-path template, cabinet/mic grid, footswitch or EXP assign, MIDI mapping, USB-audio/reamp route, or operating mode (My Presets / Favorites / Factory / Cloud / Songs / Setlists / DAW Mode / Looper) *means* in product terms — as opposed to the wire serialization (`tmp-companion-protocol`) or the catalog data contract (`tmp-companion-catalog`). Owns the 11 signal-path templates, scene-overlay semantics, the firmware-enforced constraints, per-preset settings, the screen inventory, and the MIDI chart. Grounds a Level/Copy/Songs feature so it is implemented semantically correctly (e.g. all scenes share one block list, reamp bypasses the analog Loops 1–2, presetLevel is a global multiplier while a scene is leveled on its amp's outputLevel)."
---

# TMP product-facing data model

Source: Fender's v1.7 Owner's Manual (49 pp) + Model Guide (127 pp) — the public product documentation. This skill captures what the device _models_: what a preset/scene/block _is_ in product terms. It is the domain dictionary for TMP's ubiquitous language, so the companion code named in it (`audiograph.rs` `FenderId`/`scenes[]`/`ftsw`, `leveller.rs` `presetLevel` vs per-scene `outputLevel`, `audio.rs` USB routing, `models/` catalog) is implemented with the right meaning.

**Orient first.** `CLAUDE.md` is the authoritative architecture index; `notes/protocol.md` + `notes/write-safety.md` carry the wire/write facts. This skill is the _product_ reference — when it and `CLAUDE.md` disagree, `CLAUDE.md` wins.

## Preset object

A preset = signal-path template + blocks + per-preset settings + footswitch assigns + EXP assigns + scenes.

> **Preset identity = `presetJson.info.preset_id`** (a UUID, unique per preset; verified unique across all factory presets). It is the stable join key for host-side per-preset metadata — NOT the user-editable `displayName`, NOT the positional slot. The on-device DB stores `presetJson` as PLAINTEXT JSON (the XOR+LZ4 encoding is only the exported `.preset` file). The firmware re-serializes `info` to a FIXED baseline field set (`author, created_at, displayName, preset_id, product_id, source_id, timestamp, version`) on every save — so extra keys injected into `info` survive an import/restore but are DROPPED on the first on-device edit+save (HW round-trip, fw 1.8.45). Durable host metadata therefore lives in a companion sidecar keyed by `preset_id`, not in the preset. (See `tmp-companion-write-safety`'s reference notes for why an in-place edit must preserve `preset_id`.)

| Store                       | Capacity | Addressing                                                                                                                                   |
| --------------------------- | -------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| User presets ("My Presets") | 504      | 4 MIDI banks: banks 0–2 = 128 each, bank 3 = 120 — Bank Select CC0=0..3 + PC 1..128                                                          |
| Songs                       | 200      | each = ordered preset bank for one song, sections labeled (intro/verse/chorus/solo/outro/…)                                                  |
| Setlists                    | 50       | each = up to 99 Songs in sequence                                                                                                            |
| Cloud presets               | 100      | not numbered, newest first, downloaded via the TMP Control desktop app                                                                       |
| User Block Presets          | 500      | **separate persistence store** — per-block factory-style defaults saved by the user; appear in the Add Block menu with a ▾ expand affordance |

### Signal-path templates (11)

Choose ONE template per preset, then populate. Splitter and Mixer are template-fixed — they exist at predetermined positions for parallel templates and cannot be added or removed independently.

- `Instrument Series` — single series chain
- `Instrument Parallel 1` — split → A/B → merge inside the path
- `Instrument Parallel 2` — wider parallel structure
- `Instrument + Mic/Line Series` — both inputs joined into a series chain
- `Instrument + Mic/Line Mix 1` / `Mix 2` / `Mix 3` — various mix routings
- `Instrument + Mic/Line Parallel` — fully parallel
- `Mic/Line Series` — mic/line only
- `Mic/Line Parallel 1` — mic/line with parallel paths
- `Instrument Split` — instrument feeds two outputs differently
- `Mic/Line Split` — mic/line feeds two outputs differently

### Block types

| Type                 | Notes                                                                                                                                                                                                                                       |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Combo Amp            | model + Cabinet sub-block (default cab pairing per amp; user can swap)                                                                                                                                                                      |
| Half-Stack           | model + Cabinet sub-block                                                                                                                                                                                                                   |
| Bass Amp             | model + Cabinet sub-block                                                                                                                                                                                                                   |
| Amp Head             | cab-less; allows manual cab pairing or no cab (driving a real cab via AMP/instrument-level output)                                                                                                                                          |
| Cabinet (standalone) | IR collection with mic config (see below)                                                                                                                                                                                                   |
| Effect               | one of ~150 models in 9 categories. Use the **Model Guide PDF** for Fender's user-facing names + real-unit attributions. The `tmp-companion-catalog` skill owns the shipped id→name/category catalog + the `available`/`subcategory` facts. |
| FX Loop 1–4          | physical loop placement marker                                                                                                                                                                                                              |
| Splitter / Mixer     | template-fixed, not user-addable/removable                                                                                                                                                                                                  |
| Impulse Response     | shares the 2 cabinet-category slots (`ComboHalfStackCabinetsLimit`), placement only after Loop 2 — see constraints below                                                                                                                    |

## Per-preset settings

- **Preset Volume**: 0–100% normalization
- **Input Impedance**: `Auto` (default — picks based on first active amp/effect) | `22k` | `22k+330pF` | `330k` | `330k+330pF` | `1M` | `1M+330pF` (6 explicit options simulate the input impedance the modeled amp/effect would present)
- **Signal Path Type**: one of the 11 templates above
- **Output Assign**: 3×3 matrix `[Upper Path / Lower Path / USB 1-2] × [Headphones / Output 1 / Output 2]` — independently togglable
- **Preset MIDI**: up to 5 messages sent on preset load, each = `(channel, PC#, CC#, CC value)`
- **Preset Spillover**: on/off — do delay/reverb tails continue across preset changes
- **AMP CTRL 1/2**: maps to the rear-panel AMP CTRL TRS jack (tip=AC1, ring=AC2 — two independent latching contact closures, used as a TRS-to-dual-TS insert cable)
- **Tap tempo scope**: per-preset BPM OR global tempo (Global Settings → Footswitch → Tap Tempo)

## Cabinet sub-model

Applies per amp block (combo/half-stack/bass) OR per standalone Cabinet block.

- 1 or 2 cabinets. Combos/half-stacks ship with 1 by default; "+ Add Cab" makes it 2.
- Per cab: 1 or 2 mics. "+ Add Mic" enables dual-mic.
- Per mic:
  - **Mic model** (7 options): `Condenser C414`, `Condenser M23`, `Dynamic MD421`, `Ribbon R121`, `Dynamic RE20`, `Dynamic SM7B`, `Dynamic SM57`
  - **Mic position**: **32-slot grid** = 4 vertical positions (`cap` / `cap edge` / `cone` / `cone edge`) × 8 distances (`0"` / `0.5"` / `1"` / `2"` / `3"` / `4"` / `5"` / `6"`). Each cell loads a distinct IR.
  - **Axis**: on-axis (straight) or off-axis (45° to reduce treble)
  - **Low-cut filter**: gradient 20 Hz–20 kHz
  - **High-cut filter**: gradient 20 Hz–20 kHz
- Dual-mic / dual-cab adds: **Blend** (mic1 vs mic2 mix), **Pan 1**, **Pan 2** (stereo placement of each)
- **External Cabinet** option: bypass internal IR. Exposes a **Speaker Impedance Curve (SIC)** parameter that tunes the modeled amp's interaction with a real cab connected via a non-FR solid-state power amp. Pick the SIC option appropriate for the cab type, or by ear.

The on-disk IR file naming (`{Cabinet}_{Speaker}_{mic}_{position}_{axis}_{distance}.wav`) indexes exactly this user-facing grid.

## Scenes

Up to **9 per preset** (8 footswitch-recallable + base preset).

Invariants (firmware-enforced):

- All scenes share the **same signal-path template, same block list, same block order**
- Adding a block to any scene loads it (enabled) in _all_ scenes
- Replacing a block in any scene replaces it in all scenes
- Changing block order or signal-path template affects all scenes

Per-scene differences:

- Bypass state of each block
- Per-block parameter values, gated by the per-block **Scene Edit flag**:
  - `ENABLED` (default): parameter changes apply only to the active scene
  - `DISABLED`: parameter changes are shared across all scenes
- Scene-specific Amp Control / MIDI PC / MIDI CC messages (each scene can send its own)

**Scene Change Behavior** (Global Settings → Footswitch):

- `Maintain Changes` (default) — reload preserves unsaved edits within the scene being recalled
- `Discard Changes` — reload reverts to the last-saved scene state

The serialization is a sparse diff (`ftswStates` array + scene-keyed override maps) — see `tmp-companion-protocol` / `notes/protocol.md`.

## Footswitch Assign (Effects FS mode, per preset)

8 of 10 physical footswitches are assignable (2 are fixed: FS Mode toggle, Tap/Tuner). Each assignable footswitch can carry up to **5 functions simultaneously**.

Function types:
| Type | Purpose |
|---|---|
| `ON/OFF` | toggle one or more blocks; multi-block = `MULTI` label; A/B selection via per-block Bypass switch |
| `Parameter Change` | toggle a single block parameter between two values |
| `Scene` | recall a scene |
| `MIDI CC` | send a CC message (channel + CC# + active/inactive values + latching/momentary) |
| `MIDI PC` | send a Program Change |
| `Amp Control` | drive AMP CTRL 1 or AMP CTRL 2 (tip/ring of the rear-panel TRS jack) |

Per-function fields: assigned block(s), Color (Active/A), Color (Inactive/B), Switch type (latching/momentary), custom label, **Switch Link group ID** (mutex group — only one footswitch in a group can be active at a time; useful for fast switching between two drives).

## EXP Assign (per preset)

Five expression sources, each independently configurable:

- `Toe Switch` (rear-panel TS jack — latching or momentary)
- `EXP 1` (rear-panel TRS jack — Fender Tread-Light or any 10k–500k pedal)
- `EXP 2` (rear-panel TRS jack)
- `MIDI EXP 3` — virtual, no physical jack, driven via MIDI CC 3
- `MIDI EXP 4` — virtual, no physical jack, driven via MIDI CC 4

Per source: up to **5 parameter targets**. Each target carries:

- Assigned block + parameter
- Heel value, Toe value
- **Taper**: 5 options (`slower` / `slow` / `normal` / `fast` / `faster`) — pedal-feel curve
- **Switchless Bypass**: off / heel-down / toe-down (300 ms hysteresis) — auto-bypass when pedal moves off a selected position
- Can also send External MIDI CC alongside the parameter change

**EXP Live Mode**: when enabled, TMP reads the live pedal position at preset load. Pattern: add a Volume Pedal block in every preset, enable EXP Live Mode → global volume that survives preset changes.

## Block inventory pointer

Don't duplicate the catalog in this skill — it stales on every firmware update. Source precedence is scoped per fact: the **Model Guide wins on official names, real-unit attributions, and appearance**; the firmware's **`product_profile.json` outranks both on on-device availability + menu category** (the guide and app both include unavailable leaks — the `tmp-companion-catalog` skill operationalizes this hierarchy and owns the shipped `tmp-model-guide.json`).

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
| Effects: Reverb      | ~22                      | many **convolution** — see constraint below                                                                                         |
| Effects: Dynamics    | ~11                      | compressors, gates, volume swell, slow attack                                                                                       |
| Effects: EQ          | ~10                      | 5/7/10-band graphic, 3/5-band parametric, LP/HP/notch filters                                                                       |
| Effects: Filter      | 5                        | 3 wahs + Filtron + Enigma envelope                                                                                                  |
| Effects: Pitch       | 10                       | Micro/Chromatic/Polygon/Polyvoice/Diatonic, Pedal Detune/Shifter, Virtual Capo, Granular Arpeggiator, Feedback Generator            |
| Effects: Synth       | 3                        | Cerberus + Aethon polysynths, Wavemorph                                                                                             |

## MIDI implementation

Full table: **`references/midi-cc-map.md`**.

Headline facts:

- **Preset addressing**: 504 presets across 4 banks — banks 0–2 hold 128 each, bank 3 holds 120 (see the bank table in `references/midi-cc-map.md`). Bank Select = CC 0 with value `0`/`1`/`2`/`3`. Program Change = `1..128` within the selected bank (`1..120` in bank 3).
- **Receive channel**: 1–16 or Omni (default Omni). Set in Global Settings → I/O → MIDI.
- **MIDI Out jack mode**: `Out` (TMP-generated only) | `Thru` (received-only passthrough) | `Merge` (both).
- **MIDI Receive PC/CC/Clock**: each independently enableable for the MIDI 5-pin jack and/or USB-C (default MIDI+USB).
- **MIDI Clock**: send + receive both available. **Gotcha**: when receive-clock is ON, the tap-tempo footswitch is disabled and per-preset saved tempos are overridden.
- **Channel rename**: outgoing MIDI channels can carry custom labels stored in user prefs (Global Settings → I/O → MIDI → Rename MIDI Channels).

Most load-bearing CCs for automation:

- `CC 1` / `CC 2` = EXP 1 / EXP 2 (physical jacks)
- `CC 3` / `CC 4` = MIDI EXP 3 / MIDI EXP 4 (virtual, no jack)
- `CC 7` = Master Volume
- `CC 20` = FS Mode enable
- `CC 21–29` = Effects FS 1–8 toggles
- `CC 30` = bulk FS 1–8 enable/disable (values 0–7 enable, 10–17 disable)
- `CC 64` = Tap Tempo
- `CC 65` = Toe Switch
- `CC 66` / `CC 67` = Amp Control 1 / 2 (tip/ring)
- `CC 68` = Tuner
- `CC 69` / `CC 70` = Next Song / Previous Song
- `CC 103–110` = Looper transport (REC/DUB, PLAY/STOP, 1-SHOT, UNDO, 1/2 SPEED, REVERSE, VOL UP, VOL DOWN)

## USB audio routing

TMP enumerates as a **4-in / 4-out** USB 2.0 audio interface. Sample rates 44.1 / 48 / 88.2 / 96 kHz DAW-selectable. Bit depth 32 (engine internal). Set mode in Global Settings → I/O → USB.

### Standard mode (default)

| USB Out | Source                                                                                                    |
| ------- | --------------------------------------------------------------------------------------------------------- |
| 1 / 2   | Processed stereo (instrument channel, mic/line channel, or both summed depending on signal-path template) |
| 3       | Dry instrument channel (pre-DSP, pre-Loops-1/2)                                                           |
| 4       | Dry mic/line channel (pre-DSP)                                                                            |

| USB In | Routing                                                                                        |
| ------ | ---------------------------------------------------------------------------------------------- |
| 1 / 2  | Stereo signal from DAW → assignable to OUT 1 / OUT 2 / Headphones via per-preset Output Assign |
| 3 / 4  | Disabled                                                                                       |

PRE/POST toggle in the Output Mixer controls whether USB 3/4 sends are pre- or post-fader.

> **USB Out 3 (dry instrument) has no limiter** — it carries the instrument at its actual level and **clips at 0 dBFS** for hot pickups played hard (live-confirmed). From the Mac's perspective it's input channel index 2; useful for measuring a real instrument's output level (the Tier-2 calibration path).

### Reamp mode (per-preset, manually toggled)

| USB In | Routing                                                                                                |
| ------ | ------------------------------------------------------------------------------------------------------ |
| 3      | DAW reamp track → instrument channel's **first signal-path block** (mutes rear-panel instrument input) |
| 4      | DAW reamp track → mic/line channel's **first signal-path block** (mutes rear-panel mic/line input)     |

Resets to OFF on power cycle. **Loops 1 and 2 are bypassed in reamp mode** — they're analog pre-A/D, so a USB-injected dry track can't reach them. Loops 3/4 are digital and remain active.

> **The reamp inject is NOT AGC'd** — the injected track's amplitude directly drives the block's nonlinearity. Any apparent flattening of input-level changes is the amp model's own compression, not normalization (live-confirmed via a re-amp amplitude sweep: a clean preset gave ~1.65 LU output per 6 dB input, linearizing toward −6 LU/6 dB at low drive). So a hotter instrument genuinely drives the chain harder. This is the foundation of instrument-aware leveling.

> **Footswitch-gated parameters default to OFF.** A modulation/tremolo block parameter (e.g. the '65 Deluxe Reverb's tremolo **Intensity**) can store `0` in the block's `dspUnitParameters` and only reach its real value via a footswitch **Parameter Change** function (`func:"param"` in the top-level `ftsw` array, `valueA` = engaged value). With the footswitch **disengaged** (`ftsw[N].isActive=false`, the default), the param stays at its stored 0 — so a "silent" effect in the preset JSON may be _gated off_, not absent. Don't read a block's presence as "it's audibly doing something."

## Firmware-enforced constraints

These are validation rules the firmware imposes — useful when mutating presets to know what won't be accepted. The block placement/count caps are enforced by the firmware's control app (as a set of per-restriction check functions the "can be selected" property runs on-read); the audio engine independently enforces only the CPU budget. The companion mirrors the same caps in `src-tauri/src/blockcaps.rs` / `src/views/copy/validateBlockEdit.ts` (see also `src/models/block-classification.md`). The values below are product facts (fw 1.8.45; the rule set is identical back to 1.7.75).

1. **Convolution-reverb limit — 1 per preset**. The cap is on the shared **FFT convolution engine**, broader than "reverb". Membership is an `acdCategory` union: standalone convolutions (`ACD_TMSpring63/65`, `Cathedral`, `HallOfDoom`, `EtherealHall`, room/plate/chamber) **and** amps with baked-in convolution spring reverb (Deluxe/Princeton/Twin/Super Reverb blackface/brownface — the `…CabIRConvRvb` ids). Total 20 members in 1.8.45, 16 in 1.7.75. That is why those amps ship `NoFx`/`Normal` (reverb-free) variants — to free the one convolution slot.
2. **Cabinet limit — 2 cabinet-category blocks per preset**. Combo amps, half-stacks, standalone Cabinet blocks, and IR blocks share the same 2 slots. A **dual-cab counts as 2 slots**. This is the firmware truth behind the Owner's-Manual "max 2 IR blocks" line.
3. **Glooper limit — 2 `ACD_Glooper` blocks per preset** (counted across both signal-chain rows).
4. **FX-loop coexistence — a rule, not a count.** A per-line-type slot-permission mask (Guitar path → all slots, Mic path → slots 0,1 denied, Split/Mix → all denied) plus a pairwise coexistence matrix (the stereo `FxLoop3_4` excludes individual `FxLoop3`/`FxLoop4`) gate whether a candidate FX-loop block may sit at its slot. The "loops 1–2 before A/D, loops 3–4 after loop 2" ordering is **structural, not a runtime check**: Loops 1–2 are rear-panel fixed loops (not add-block candidates; only `ACD_FxLoop3/4/3_4` have selectable profiles) and the Mic mask denies slots 0–1.
5. **Processor utilization — CPU budget, no fixed block count.** The **Add Block menu greys out** when adding a block would exceed the per-preset budget (76.5%, summed per-block `utilizationPercentage`; the numbers ship in `src/models/model-cpu.json`). Rejection strings: `Can't add/insert/replace node to guitar/mic group: over cpu budget`. This is the real "path is full" cap; the count limits above stack on top of it.
6. **Loops 1 and 2 are fixed at the start of the Instrument path, BEFORE A/D**. They cannot be moved or placed in mic/line paths — they're for analog pedals that need to interact with pickup impedance (fuzz, Rangemaster-style boost, vintage wah).
7. **Loops 3 + 4** pair as a single stereo loop OR two mono loops — per-preset choice (`Loop 3 mono`, `Loop 4 mono`, `Loop 3+4 stereo`). Placeable anywhere in the digital signal path after Loop 2. Both inputs cannot be set to off (`Both inputs cannot be set to off.`).
8. **IR blocks placement**: only after Loop 2 (because they're digital). Count is governed by the cabinet limit above.
9. **Scenes share blocks**: all scenes in a preset share the same block list, same block order, same signal-path template. Adding/removing/reordering blocks affects all scenes uniformly. Scene slots are capped (`All scene slots full`).
10. **Splitter/Mixer are template-fixed**: they appear at predetermined positions for parallel templates and cannot be user-added or removed independently. To change parallel topology, change the signal-path template (which repopulates the path).
11. **Other capacity caps** surfaced as firmware rejections: `Cannot add new user preset in populated slot.`, saved block-presets (`BlockPresetLimitReached`), cloud/downloaded presets (`Downloaded Presets Limit Reached`).

## Operating modes

Six navigation modes via the left-side touchscreen icons:

| Mode            | Capacity | Behavior                                                                                                                                                                                                                                                                                                                           |
| --------------- | -------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| My Presets      | 504      | user-editable; drag-and-drop reorderable on touchscreen; reachable via MIDI Bank+PC                                                                                                                                                                                                                                                |
| Favorites       | subset   | star-marked subset of My Presets; separately reorderable but keeps original preset number                                                                                                                                                                                                                                          |
| Factory Presets | factory  | unnumbered, not directly editable; load → modify → "Save to My Presets"                                                                                                                                                                                                                                                            |
| Cloud Presets   | 100      | downloaded via the TMP Control desktop app; newest first; not numbered                                                                                                                                                                                                                                                             |
| Songs           | 200      | each = up to 6 presets with labeled sections (intro/verse/chorus/solo/outro/…); per-song BPM available (wire mechanism: no dedicated setter — it's the global `SettingsMessage.tapTempoBpm` applied to the active song; song/setlist CRUD is `SongMessage`/`SetlistMessage` field-numbered setters — see `tmp-companion-protocol`) |
| Setlists        | 50       | each = an **ordered** list of up to 99 Songs (position matters); a song may belong to **many** setlists; add / remove-from / reorder-within a setlist are all supported (wire: `addSetlistSong` (global slot) / `removeSetlistSong` / `moveSetlistSong` (1-based position) — see `tmp-companion-protocol`)                         |

The `tabEnum` wire encoding for these tabs is `NotSet=0`, `UserPresets=1`, `FavoritePresets=2`, `CloudPresets=3`, `FactoryPresets=4`, `Songs=5`, `Setlists=6`. Product note: the cursive "F" top-bar badge is the **Factory** badge (`tabEnum=4`), not Favorites — it means "this preset came from the factory tab, so the badge shows the brand mark instead of a numeric slot". My-Presets selections (`tabEnum=1`) render the slot number ("01", "02", …).

**DAW Mode** (separate, modal): hold `FS Mode` + `Tap Tempo` footswitches for 2 s. Footswitches rebind to **Fender Studio Pro** transport — `Play` / `Stop` / `Record` / `Return to Zero` / `Click` / `Pre-Roll` / `Punch In` / `Loop`. Press `EXIT` footswitch to leave.

**Looper** (also modal): hold `FS Mode` for 2 s. Footswitches rebind to `LOOP VOL UP/DOWN`, `UNDO`, `1/2 SPEED`, `REVERSE`, `EXIT`, `RECORD/OVERDUB`, `PLAY/STOP`, `1-SHOT`, `TAP TEMPO`. Stereo loops up to 3 min at full speed. Tap tempo and tuner remain accessible. Loop playback continues across preset changes.

## Screen inventory & UI surfaces

The firmware exposes the following screens / modals. Each is a _product surface_ — a defined view the user can be inside.

| Surface                | Purpose                                                                                                                                                                      |
| ---------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `List View`            | scrollable preset/song/setlist list (any of the 6 operating modes)                                                                                                           |
| `Preset View`          | current preset's signal chain, upper ribbon (List View / star / number box / Save / gear), lower ribbon (EXP Assign / Footswitch Assign / Preset Settings / Add Block / Tap) |
| `Gig View`             | fullscreen preset name + number (preset mode) OR song list (Songs/Setlists mode) — minimal, accident-resistant performance view                                              |
| `Block Edit`           | zoomed-in view of one block with 6 visible parameters + PAGE footswitch for additional pages                                                                                 |
| `Cabinet Settings`     | 32-position mic grid + cab/mic selectors + axis/filters + dual-cab/dual-mic Blend+Pan + External Cabinet + SIC                                                               |
| `Add Block` menu       | category list + model list with audition + Block Preset expand                                                                                                               |
| `Move/Delete`          | block reorder mode (long-press triggered), drag-to-bottom to remove                                                                                                          |
| `Footswitch Assign`    | 8-footswitch panel for editing Effects FS mode assignments                                                                                                                   |
| `EXP Assign`           | 5-source panel for editing pedal/toe/MIDI-EXP parameter targets                                                                                                              |
| `Preset Settings menu` | Preset Volume / Signal Path Type / Input Impedance / Output Assign / Preset MIDI / Preset Spillover / Amp Control                                                            |
| `Save dialog`          | name field + Save Location list + "select next empty preset" shortcut                                                                                                        |
| `Global Settings`      | gear-accessed, with 7 bottom tabs: Preferences / I/O / Footswitch / Bluetooth / EQ / Mixer / Tuner                                                                           |
| `Tuner`                | full-screen chromatic tuner with reference frequency + mute toggle + INSTRUMENT/MIC-LINE selector                                                                            |
| `Mixer`                | per-output faders (Headphones / OUT 1 / OUT 2 / USB 1-2 / USB 3 / USB 4) with AUX, Bluetooth, Mute, Solo, PRE/POST                                                           |
| `Looper`               | modal — looper transport footswitch layout (hold-2s entry)                                                                                                                   |
| `DAW Mode`             | modal — Fender Studio Pro transport footswitch layout (hold-2s entry)                                                                                                        |

## Why this matters for the companion

- **Copy** (`src/views/copy/copyModel.ts` + `audiograph.rs`): a preset is a signal-path template + block list + per-block `(model_id, params, bypass, scene_edit_flag)` + scenes (a sparse bypass+parameter overlay) + footswitch/EXP assigns. Because **all scenes share one block list**, a Copy insert/remove must land in every scene, and the block lives in three keyed places (see `tmp-companion-write-safety`).
- **Leveling** (`leveller.rs` / `audio.rs`): `presetLevel` is a **global multiplier** over all scenes → level the base scene first; each footswitch scene is leveled on its **active amp's `outputLevel`**. Reamp routes the DAW track into the chain's first block and bypasses the analog Loops 1–2, and the inject is not AGC'd — the model above is why.
- **Signal chain + Catalog** (`SignalChainView` / `models/`): the 11 templates + block types + the cabinet sub-model are what the strip renders; `tmp-companion-catalog` owns the id→art/name mapping.

## Sources

- `Tone Master Pro` Interactive Owner's Manual (firmware v1.7, 49 pp)
- `Tone Master Pro` Model Guide (firmware v1.7, 127 pp)

Re-fetch from Fender's product page when firmware revs (new models, MIDI-map changes, capacity-cap changes). Firmware 1.8 ships 31 new models, so this v1.7-pinned snapshot is one generation behind on the model inventory — the structural model above (templates, scenes, footswitch/EXP, constraints) is stable across 1.7→1.8.
