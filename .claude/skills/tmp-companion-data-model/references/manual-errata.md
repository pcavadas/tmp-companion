# Owner's Manual errata

Source: Owner's Manual v1.8 (rev. J), read end to end (text layer + every page rendered as an image).
Page refs are **printed** pages.

Where the manual contradicts itself or its own artwork.
Device behaviour the manual leaves open is stated in the other references and in [`notes/device/`](../../../../notes/device/).

---

## E. Manual's own internal inconsistencies

Confirmed as genuinely printed — not extraction artefacts.

| Topic | Conflict |
|---|---|
| **EXP pedal impedance** | p.38 prose: **10 k–500 kΩ**. p.46 spec table: **1 kΩ–500 kΩ**. |
| **Master volume scope** | p.3: controls "all outputs". p.36: `MASTER` assign exists on only three of six mixer channels — the knob does not reach USB 1/2 (`global-settings.md` §6). |
| **Block parameters per page** | p.13 prose: "up to six … assigned to the middle six footswitches". p.13 screenshot: **seven** bound, including `AMP LEVEL` in the bottom-left slot. |
| **Mic position order** | p.16 prose lists "cap, cap edge, cone, cone edge". The matrix renders top→bottom as **CONE EDGE, CONE, CAP EDGE, CAP** — the prose order read bottom-up. |
| **Add Block cancel control** | p.15 prose: "touching CANCEL at upper left". Screenshot: the control reads **`< Back`**. |
| **Output Assign column order** | p.17 prose lists "OUTPUT 1, OUTPUT 2 and HEADPHONES". The screen orders them **Headphones, Output 1, Output 2**. |
| **USB 3/4 Pre/Post** | p.36 prose describes one choice "out USB 3/4". The screen gives **USB 3 and USB 4 each an independent PRE/POST pair**. |
| **Splitter vs mixer symbol** | p.12 prose implies two distinguishable symbols; both the margin art and the p.18 diagram render the **same** 3-fader glyph. |
| **Firmware restart wording** | p.44 prose implies restart follows completion; the device screen reads "Please restart to continue applying update." |
| **Gig View scope** | p.6 groups Cloud Presets among "the first four Preset modes"; p.7 lists Gig View as available in **My Presets, Favorites and Factory Presets** only — Cloud omitted. |
| **MIDI copy-paste slips** | p.34 "SEND MIDI PC/CC … sets tempo for external devices that respond to MIDI clock" (clock text duplicated), and "RECEIVE MIDI CC: Select to receive MIDI **preset changes**" (should be continuous-controller messages). |
| **Tuner mute wording** | p.32: "The tuner mutes **output** by default; tuner mutes only **input** being tuned." |
| **Device screen typo** | The firmware-update screen prints `www.fender/tonemaster_pro`, missing `.com`. Genuinely on the device. |

---

## F. Documentation artefacts to be aware of

### F1. The DAW Mode page has "ghost" text — **do not use its text extraction**

The PDF text layer for p.45 contains strings that are **not rendered on the page**: `DAW 99 Studio Pro`, `FOOTSWITCH ASSIGNMENT`, `VOLUME UP`, `VOLUME DOWN`, `MUTE TRACK`, `SOLO TRACK`, `ARM TRACK`, `PLAY STOP`, and an instruction bar reading "SELECT FOOTSWITCH TO **TOGGLE** …".
They describe a different, track-oriented DAW layout the published manual does not show.

The **visible** layout is authoritative, and is corroborated three ways: the rendered screenshot, the page's own prose naming exactly those eight transport functions, and the tell that the real equivalent instruction bar on p.27 says "TO **ACTIVATE**".
See `device-operation.md` §5.

### F2. The signal-path thumbnails are schematic

The p.18 template icons draw every element as an identical square and do **not** distinguish a terminal (`INSTRUMENT` / `OUTPUT`) from a block.
Square counts are not block limits and not element inventories.

### F3. The tuner screenshot is a composite

p.32 shows yellow bars lit on both sides of centre while reading `+1` cents — not a physically possible single-pitch state.
Illustrative only.

### F4. Model inventory is out of scope here

The manual defers all per-model amp/cab/pedal/mic description to the separate **Model Guide**.
This repo bundles it as `src/models/tmp-model-guide.json`; `tmp-companion-catalog` owns the id→name/category contract, and `product_profile.json` outranks both on on-device availability.
