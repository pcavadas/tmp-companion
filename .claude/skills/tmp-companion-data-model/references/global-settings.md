# Global Settings — complete inventory with defaults

Source: Owner's Manual v1.8, printed pp. 32–36 (+ p.7, 27, 42 for cross-scope rules). Page refs are **printed** pages.

Reached via the **gear** at upper right. Seven categories on a persistent bottom nav bar, in this on-screen order:

`Preferences` · `I/O` · `Footswitch` · `Bluetooth` · `EQ` · `Mixer` · `Tuner`

The bar is global chrome — it stays visible inside the EQ, Mixer and Tuner screens.

**Everything here is device-global** unless a row says otherwise. For what is per-preset, per-scene or per-block, see `SKILL.md`; for the override precedence between the two, see §8 below.

---

## 1. Preferences (p.33)

| Setting                                        | Default | Behaviour                                                                                  |
| ---------------------------------------------- | ------- | ------------------------------------------------------------------------------------------ |
| About                                          | —       | shows installed firmware version                                                           |
| Retain Global EQ                               | **OFF** | when OFF, Global EQ **reverts to flat at every power-up**; ON persists it                  |
| Wrap-around signal path                        | —       | whether **single-input** paths wrap onscreen instead of scrolling off to the right         |
| Swipe up/down to change preset                 | —       | enables preset scrolling in Preset View; settable **independently on hardware and mobile** |
| Seamless tap tempo delay                       | —       | when enabled, delays no longer "pitch warp" as tap tempo changes delay time                |
| Auto display brightness                        | —       | ambient-light sensor                                                                       |
| Display brightness                             | —       | one slider covering touchscreen **+ scribble strips + footswitch LEDs**                    |
| Backup presets, settings and IRs to SD card    | —       | ≤ **25** named backups; card not included                                                  |
| Restore presets, settings and IRs from SD card | —       | separate menu item                                                                         |
| Factory Reset                                  | —       | restores **all presets and settings** to factory spec                                      |

> There is no screenshot of this menu anywhere in the manual, so no image-sourced values exist for it.

---

## 2. I/O (p.33–34)

Displays the rear panel at left with five subcategories.

### Inputs

| Setting                      | Default        | Notes                                                                 |
| ---------------------------- | -------------- | --------------------------------------------------------------------- |
| Instrument input pad (−6 dB) | **OFF**        | for instruments with active preamps; raises max input 11.2 → 17.2 dBu |
| Mic input gain               | **+9 dB**      | "typical for a standard dynamic microphone"                           |
| Line input gain              | **0 dB**       |                                                                       |
| +48 V phantom power          | **OFF**        | XLR only. Disable before connecting XLR to a mixer/interface (p.38)   |
| Loop 3/4 level               | **INSTRUMENT** | "typical for stompbox pedals"; choose LINE for rack effects           |

### Outputs

| Setting        | Default                                                     |
| -------------- | ----------------------------------------------------------- |
| Output 1 level | **LINE** — choose INSTRUMENT when feeding an instrument amp |
| Output 1 mode  | **STEREO**                                                  |
| Output 2 level | **LINE**                                                    |
| Output 2 mode  | **STEREO**                                                  |

### EXP / Ctrl

| Setting               | Default                                    |
| --------------------- | ------------------------------------------ |
| EXP 1/2 polarity      | **NORMAL** (REVERSE for pedals needing it) |
| Toe switch            | **LATCHING**                               |
| Amp Ctrl 1/2 polarity | **NORMAL**                                 |

### USB

| Setting    | Default                                        |
| ---------- | ---------------------------------------------- |
| Reamp mode | **OFF** — and **resets to OFF on power-cycle** |

### MIDI

Full chart in `midi-cc-map.md`. Settings and defaults:

| Setting              | Default        | Options                                                                               |
| -------------------- | -------------- | ------------------------------------------------------------------------------------- |
| MIDI Out             | **OUT**        | `OUT` generated-only · `THRU` received-only · `MERGE` both                            |
| Receive channel      | **OMNI**       | 1–16 or Omni                                                                          |
| Receive MIDI Clock   | **OFF**        | when ON, the **tap footswitch is disabled** and stored preset tempos are ignored      |
| Receive MIDI PC      | **MIDI + USB** | per-transport                                                                         |
| Receive MIDI CC      | **MIDI + USB** | per-transport                                                                         |
| Send MIDI Clock      | **OFF**        | to MIDI jack and/or USB                                                               |
| Send MIDI PC/CC      | **MIDI + USB** |                                                                                       |
| Rename MIDI channels | —              | custom names for each **outgoing** channel; appear in all preset/scene MIDI workflows |

---

## 3. Footswitch (p.34)

Global, and applies across **Presets, Songs and Setlists** modes.

| Setting                                             | Default              | Options                                                                                                   |
| --------------------------------------------------- | -------------------- | --------------------------------------------------------------------------------------------------------- |
| FS Mode: Presets                                    | **6 presets**        | 6 presets · 3 effects over 3 presets · 3 presets over 3 effects                                           |
| FS Mode: Effects                                    | **6 effects**        | 6 effects (bank up/down retained at far left) · **8 effects** (the bank switches become effects switches) |
| Tap Tempo                                           | **PRESET**           | `PRESET` saves tempo per preset · `GLOBAL` one tempo overriding every preset's stored tempo               |
| Tap LED                                             | **ON**               | `ON` always flash at tempo · `MOMENTARY` flash 5 s then stop · `OFF` never                                |
| **Scene Change Behavior**                           | **MAINTAIN CHANGES** | see below — load-bearing                                                                                  |
| Exit tuner on preset change                         | —                    | `ON` exits on EXIT Tuner **or any preset footswitch** · `OFF` only the EXIT tuner footswitch exits        |
| Default preset footswitch colours (active/inactive) | —                    | overridden per preset                                                                                     |
| Default footswitch assign colours (active/inactive) | —                    | applies to new assignments                                                                                |
| Song footswitch layout (auto load)                  | **6 presets**        | same three layouts, plus preset auto-load options when a Song is selected                                 |
| Song custom label display                           | —                    | `LABEL + PRESET NAME` or `LABEL ONLY`                                                                     |

### Scene Change Behavior — the one to watch

How a scene reloads **after it has been modified but not saved**:

- **`MAINTAIN CHANGES` (default)** — reloads the scene in its last modified state. Lets you edit several scenes and save once.
- **`DISCARD CHANGES`** — reloads exactly as last saved. The manual warns explicitly: when creating or editing scenes under this setting, **the preset must be saved before switching scenes or the changes are lost.**

Any host-side workflow that accumulates unsaved per-scene writes across scene recalls and saves once at the end is only valid under `MAINTAIN CHANGES`.

> The MODE footswitch toggles Presets ⇄ Effects. With a second assignment page present the strip reads `FS: FX PAGE 1`, and it cycles page 1 → page 2 → back to Preset mode.

---

## 4. Bluetooth (p.35)

| Setting     | Default                                          |
| ----------- | ------------------------------------------------ |
| Bluetooth   | **OFF**                                          |
| Device name | — (pop-up keyboard; appears in the pairing list) |

Audio only. Routable to Headphones / Output 1 / Output 2 / **USB 1/2** from the mixer.

---

## 5. Global EQ (p.35)

A **10-band graphic EQ plus a volume fader** — eleven faders total.

**Band centre frequencies, left → right (image-sourced; not in the prose):**

`31.25` · `62.5` · `125` · `250` · `500` · `1k` · `2k` · `4k` · `8k` · `16k` Hz, then `VOL`.

**Gain grid is labelled ±12 dB with 3 dB gridlines**: `+12 / +9 / +6 / +3 / 0 / −3 / −6 / −9 / −12 dB`.

- **Three built-in profiles** rendered as **curve glyph buttons**, not words, lower left in this order: flat (default) · high cut (descending shelf) · low cut (ascending shelf).
- **Four user slots** — buttons `USER 1` … `USER 4` with `SAVE` to their right in the same row. The `⋯` menu names a saved setting.
- **Output assignment**: `OUTPUT 1` and `OUTPUT 2` are two **separate, independently toggled** buttons stacked at the far right.

Remember `Retain Global EQ` defaults **OFF** — the EQ returns to flat on every power-up unless you turn that on.

---

## 6. Output Mixer (p.36)

**Six channels, left → right:** `HEADPHONES` · `OUTPUT 1` · `OUTPUT 2` · `USB 1/2` · `USB 3` · `USB 4`

Every channel has a fader, a meter, `M` (mute) and `S` (solo). What differs is the row above:

| Channel                              | Extra controls                                     | Meter               |
| ------------------------------------ | -------------------------------------------------- | ------------------- |
| `HEADPHONES`, `OUTPUT 1`, `OUTPUT 2` | `MASTER` assign · `AUX` · Bluetooth                | **2 bars (stereo)** |
| `USB 1/2`                            | `AUX` · Bluetooth _(no MASTER)_                    | **2 bars (stereo)** |
| `USB 3`, `USB 4`                     | `PRE` / `POST` _(no AUX, no Bluetooth, no MASTER)_ | **1 bar (mono)**    |

- **MASTER assign** exists **only** on the three physical outputs, and each is toggled independently. Assigned faders move in unison with the master volume knob; unassigned ones are independent.
- **`PRE` / `POST` is per-channel** — USB 3 and USB 4 each have their own pair, both defaulting to `PRE`. (The prose describes it as a single choice "out USB 3/4"; the UI shows two.) `PRE` sends the dry input at 0 dBFS reference, fader-independent; `POST` makes the fader affect it.
- **Solo is additive** — soloing mutes the others, but touching additional solo buttons adds those channels back into the mix.
- **AUX** and **Bluetooth** buttons inject those sources into the selected channel — including `USB 1/2`.
- **Meter scale, top → bottom:** `0 / −5 / −10 / −15 / −20 / −30 / −40 / −50 / −60 dB` (screen says `dB`; prose says dBFS). Red at top = clipping, yellow 0 → −20, green below −20.

> Mixer state is **global and invisible to a preset**. Anything measuring or capturing `USB 1/2` is downstream of that channel's fader, mute and solo — see `open-questions.md` on whether the master volume knob also reaches it.

---

## 7. Tuner (p.32, 36)

Reachable from Global Settings → Tuner, or by holding TAP/TUNER for 2 s.

| Element            | Detail                                                                                                       |
| ------------------ | ------------------------------------------------------------------------------------------------------------ |
| Modes              | `Needle` + **three** Strobe modes, chosen from a **dropdown** at top right showing the current mode          |
| Cents readout      | large, **signed** (`+1`), top centre                                                                         |
| Meter range        | explicitly scaled **−50 … +50 cents**                                                                        |
| In-tune indication | centre bar ± one bar turn **green within ±3 cents**; yellow bars show degrees of sharp (right) / flat (left) |
| Note names         | **flats only** — displays E♭, never D♯                                                                       |
| Reference          | **A440** default, adjustable **A430–A450**. The button's label _is_ the current value (`A440`)               |
| Mute               | **on by default**; mutes only the input being tuned. `MUTE` pill at lower left                               |
| Input select       | `INSTRUMENT` / `MIC/LINE` pills at lower right — independently togglable, so either or both                  |
| Exit               | select any preset, or press EXIT TUNER (subject to `Exit tuner on preset change`)                            |

> The p.32 screenshot shows yellow bars lit on _both_ sides of centre at `+1` cents — a physically impossible state. It is an illustrative composite; do not infer lit-bar behaviour from it.

---

## 8. Cross-scope override rules

Where a global setting and a more local one both apply:

| Contest                                                          | Winner                                                                                    |
| ---------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| Global `Tap Tempo: GLOBAL` vs per-preset stored tempo            | **global**                                                                                |
| **Song BPM** (per song, default off) vs preset tempos            | **Song BPM**, over all presets in that song                                               |
| **MIDI Clock receive** (global) vs everything                    | **MIDI clock** — tap footswitch disabled, stored preset tempos ignored                    |
| Global `Tap Tempo: GLOBAL` vs **Song BPM**                       | **UNDOCUMENTED** — both are described as overriding preset tempo; see `open-questions.md` |
| Global default footswitch colours vs per-preset colours          | **per-preset**                                                                            |
| Global default footswitch colours vs per-song assignment colours | **per-song assignment**                                                                   |
| Global EQ vs preset volume                                       | independent — Global EQ is a global output-stage EQ assignable per output                 |
| Global mixer faders vs Preset Volume                             | independent, and they compound                                                            |
