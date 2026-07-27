# On-device workflows — how the user actually does things

Source: Owner's Manual v1.8. Page refs are **printed** pages.

`SKILL.md` says what the objects _are_; this file says what the user _does_. Use it to answer "how do I…" questions, to mirror device affordances in companion UI, and to know what state a user action leaves the unit in.

---

## 1. Presets

### Create

Touch **Create Preset** (`+`) at the top of the preset list — it jumps to the **first empty preset slot**. Or turn the navigation control to any empty slot. An empty signal path appears, ready to populate. If none are free: _"All user preset locations are full."_ (p.9)

### Clear

More Options (`⋯`) beside the preset name → **CLEAR PRESET**. This removes all blocks, **resets the name to `EMPTY`**, and **resets all preset settings to default**. (p.9)

### Save (p.25)

A modified preset turns its **number box from blue to red** — the only unsaved-state indicator. Touch **SAVE** at top right. The save sheet has:

- **Preset name** — a text field with a `clear` control inside it; tap to edit via pop-up keyboard.
- **Save Location in My Presets**, legended `(*current preset)`. The current preset's row is prefixed `*` and highlighted.
- **`select next empty preset`** — jumps to the first available slot.
- Footer: grey `CANCEL`, red `SAVE`.

**Shortcut:** while editing block parameters, **press and hold the `PAGE` footswitch** (upper left) to save. (p.13)

Two rules that bite:

- **Presets always save to My Presets**, whether they came from My Presets, Factory or Cloud.
- **There is no save prompt.** Navigating away from an edited preset does not ask you to save. _(The manual states only that it does not prompt — it does not say the edits are discarded, and `Scene Change Behavior: MAINTAIN CHANGES`, the default, argues they persist in some form. See `open-questions.md`.)_

### Reorder, favourite, search

- **Reorder:** hold a preset for **one second**, then drag. This **renumbers presets automatically**. (p.9)
- **Favourite:** touch the star by the preset number. It turns blue and the preset is added to the **top** of Favorites. Favorites reorder independently but **keep their original numbers**. Touch again to remove. (p.9)
- **Search:** the magnifier searches **within the current mode only**. (p.9)
- Turning the navigation control **loads** the next/previous preset — it does not merely highlight it. (p.9)

---

## 2. Blocks

### Add (p.15)

Touch **Add Block** in the lower ribbon → `⊕` nodes appear at every legal insertion point, including the head and tail of the chain → touch a node → the Add Block menu opens with categories at left, models in the body.

- **`Confirm`** (upper right, blue) places the block. The upper-left control reads **`< Back`** (the prose calls it CANCEL).
- Touch a model to **audition it in the signal path** before committing.
- **Search** covers block names, block-preset names and effect type **within the current category**.
- **`☆ Favorites`** toggle filters categories to favourited blocks; **the filter persists until turned off**.
- Models grey out when no more can be added (the CPU budget — see `SKILL.md` constraint 5).

### Edit (p.13)

Touch any block to zoom into it. The screen shows the modelled faceplate with its knobs, plus — **outside** the faceplate art, as label + value tiles — screen-level controls such as `Amp Level` and `Noise Gate`. These are a different class from modelled knobs.

Adjusting a control: touch and slide up/down; tap a switch to toggle; or **single-tap a knob** for a gradient slider at the right of the screen with **`+`/`−` fine-tune buttons** below it. Touch elsewhere to close.

**By foot:** a block's visible parameters bind to the footswitches, each value shown in the scribble strip above; **turn the footswitch** to change it. (The prose says "up to six … to the middle six footswitches"; the p.13 screenshot binds **seven**, with `AMP LEVEL` occupying the bottom-left slot that is `BANK DOWN` in preset mode.)

**More than a screenful of parameters:** page dots appear beneath the controls showing how many extra pages exist. Touch the dots to advance; **touching the dots on the last page wraps to the first**. The `PAGE` footswitch (upper left) is the alternative.

Lower ribbon while editing: `BYPASS` · `⋯ Block Settings` · `⇄ Replace` · `🗑 Remove`. A bypassed block renders **greyed out** and the BYPASS button turns blue.

**Swiping between blocks is path-scoped.** Swipe left/right moves to adjacent blocks — but **instrument and mic/line paths are completely separate for swiping**. To reach a mic/line block you must exit edit mode and select one in that path. For parallel paths, swipe order follows the equivalent series order (left/right, top/bottom).

### Move and delete (p.14)

Drag a block to any available `⊕` node; the drop target renders as a filled blue square. Drag it to the **bottom of the screen** to remove — the bottom bar turns solid red when the block is over it. Double-tap a block in Preset View to toggle its bypass. (p.10)

### Block Presets — reusable per-block settings (pp. 13, 15, 16)

**Create:** adjust the block's parameters → **Block Settings** (`⋯`) → **Save Block Presets** → type a name → **Confirm**. It now appears in the model list in the Add/Replace menus.

**Use:** a **`▼` caret** beside a block name means it has user Block Presets. Expanding shows up to **five**, and **the first item in the list is the factory default**. Touch one to audition it in the signal path. The `⋯` menu renames or deletes it. Drag-and-drop reorders them within the Add/Replace menus.

Capacity is **500 Block Presets** device-wide. User block presets for **dual cabs** appear at the **top** of the cabinet list.

---

## 3. Cabinets and microphones (pp. 16–17)

**Access:** on a combo or half-stack amp block, touch the **small cabinet image at upper right** of the block-edit screen. Touching a standalone Cabinet block gives the same screen.

**The mic position matrix is 4 × 8 = 32 cells.** Rows render **top → bottom as `CONE EDGE`, `CONE`, `CAP EDGE`, `CAP`** (the prose lists them bottom-up). Columns are distances: `0"` `.5"` `1"` `2"` `3"` `4"` `5"` `6"`. Each cell loads a **distinct IR**.

Control tiles: cabinet · mic model · `AXIS` (value renders as `ON` for on-axis; off-axis points at 45° and reduces treble) · `LOW CUT` · `HIGH CUT`. **Filter defaults are `20Hz` / `20kHz`** — fully open.

- **`+ Add Mic`** — a second mic on a combo / half-stack / bass cab. Each mic gets its own model, position, axis and filters; a `Mic 1` / `Mic 2` selector appears.
- **`+ Add Cab`** — a second cab on a standalone cabinet block, likewise, with `Cab 1` / `Cab 2` tabs. A dual cab renders as **one parallel block** and moves/deletes as a unit.
- Either way a right-hand panel appears with **three knobs: `Blend`, `Pan 1`, `Pan 2`** — one blend and **two independent pans**, not "blend and pan".
- With Cab 2 / Mic 2 selected, the control tiles' category labels are suffixed `2` (`CABINET 2`, `MIC 2`, `AXIS 2`, `FILTER 2`).
- Both positions display on the one matrix simultaneously, **colour-coded** (Cab 1 blue, Cab 2 magenta).

**External Cabinet** sits at the **bottom of the cabinet select list** — for driving a real (non-FR) cab from a solid-state power amp. It exposes a **Speaker Impedance Curve (SIC)** that changes how the amp model interacts with the connected cab.

---

## 4. Footswitch assignments (pp. 19–23)

**Access:** Preset View → **Footswitch Assign** in the lower ribbon.

The screen is a 5 × 2 virtual grid. Columns 1–4 of both rows are the **eight assignable positions**; column 5 top is `< PAGE 1/2 >` and column 5 bottom is inert. **Two pages = 16 assignments per preset.**

**Assign:** touch any scribble strip showing `+` → the **Select Assignment Type** list appears, in this exact order: `ON/OFF` · `PARAMETER CHANGE` · `SCENE` · `LOOPER` · `MIDI` · `AMP CONTROL` → select block(s) where applicable (selected blocks get a blue arrow beneath) → `CONFIRM`.

**Shortcut:** press and hold an **empty** assignment footswitch for **two seconds** to jump straight to the type picker.

**Reorder / remove:** drag-and-drop to swap with another footswitch; drag **right** to move to page 2; drag to the **bottom** to remove. The on-screen instruction bar reads: _"SELECT FOOTSWITCH TO TOGGLE. SELECT FOOTSWITCH DISPLAY TO EDIT. DRAG & DROP FOOTSWITCH TO REORDER."_

Touching an on-screen footswitch toggles it between active and inactive so you can preview colours.

### The settings screen

Left pane: the footswitch with its LED ring, and a **five-slot stack** — one slot per function, `+` to add. Right pane rows, with observed defaults:

`Type` → e.g. `ON/OFF` · `Block` → e.g. `GREENBOX 8` · `Color (Active/Inactive)` → `RED/DIM` · `Switch` → `LATCHING` · `Custom Label` → _(empty, shows `+`)_ · `Switch Link` → `OFF`. A trash can deletes the assignment.

Field scope (which are switch-level vs per-function) is in `SKILL.md`.

### Per type

- **ON/OFF** — any number of blocks, toggled together. By default a block's on/off state stays in sync with the footswitch LED. **To make an A/B toggle:** assign both blocks, return to Preset View, then **bypass one of them** — the switch now alternates between them. Useful for clean/dirty amp pairs.
- **PARAMETER CHANGE** — exactly **one** block. Touch the block → `CONFIRM` → the **Select Parameter** screen → touch the parameter → set the **active and inactive values** on a gradient slider, audible live. Example from the manual: reverb mix 50 % active / 15 % inactive.
- **SCENE** — recalls a scene; see `SKILL.md`.
- **LOOPER** — Record/Overdub, Play/Stop, Reverse, ½ Speed, 1-Shot, Undo, or **EZ Looper**. EZ Looper's state machine: **Record → Play → Overdub → Play …**; **from Play, double-tap to Stop**; **from Stop, press and hold to erase**.
- **MIDI** — CC or PC. Configurable: MIDI channel, PC number, CC number, **active and inactive values**, active/inactive LED colours, momentary/latching, custom label.
- **AMP CONTROL 1 / 2** — the rear-panel contact closures. Configurable: colours, switch type, custom label.

Combining functions on one switch displays as **`MULTI`** in the scribble strip (renameable via Custom Label).

---

## 5. EXP and toe switch (p.24)

**Access:** Preset View → **EXP Assign** in the lower ribbon. Five columns: `TOE SWITCH` · `EXP 1` · `EXP 2` · `MIDI EXP 3` · `MIDI EXP 4`, each stacking up to **five** assignments under a `+`.

- **Toe Switch:** choose a block from the signal path → `CONFIRM`. Up to five blocks toggled.
- **EXP 1–4:** choose a block → `CONFIRM` → **Select Parameter** → set **Heel** and **Toe** values on a gradient slider. Can instead send an **External MIDI CC**.

Observed defaults in the assignment editor: `Heel` **0 %**, `Toe` **100 %**, `Taper` **NORMAL**, `Switchless Bypass` **OFF**.

- **Taper** — `slower` · `slow` · `normal` · `fast` · `faster`. A slower taper gives smoother volume swells.
- **Switchless Bypass** — the assigned block auto-engages when the pedal leaves a chosen rest position and disengages after **300 ms** back at rest. `off` / `heel-down` / `toe-down`.
- **EXP Live Mode** — on preset change, read the pedal's **live physical position**. The documented use: put a Volume Pedal block in every preset and enable this for a global volume pedal.

Classic wah recipe: toe switch toggles the wah on/off, EXP controls its position.

---

## 6. Songs (pp. 26–27)

### Create

Songs mode → **`+`** at upper right → name via keyboard → `CONFIRM`. The **Song Footswitch Assignment** screen appears: a 5 × 2 grid where **columns 2–4 of both rows are the six assignable slots** (column 1 is PREV/NEXT SONG, column 5 top is the MODE switch).

**Assign a preset or scene:** touch a slot showing `+` → the **Add Preset to Song** screen (searchable) → `CONFIRM`. A **Scene** can be assigned, not just a whole preset.

**Shortcut:** hold an **empty** song footswitch for **two seconds** to jump to Add Preset To Song.

Each assignment's editor has exactly three rows: `Preset` · `Label` · `Color (Active/Inactive)` (default **`DEFAULT/DEFAULT`**). Labels come pre-written for song sections — intro, verse 1, chorus 1, solo, verse 2, outro. These colours **override the global default preset footswitch colours**.

**Reorder:** drag-and-drop a slot; if the destination is occupied the two **swap**.

### Song BPM — "how do I set the tempo of a song"

The **Song BPM** control occupies the **bottom-right cell** of the assignment grid and reads `SONG BPM` over `OFF` when disabled.

1. **Touch the virtual footswitch** to enable Song BPM.
2. **Touch the virtual scribble strip** to set the value with a gradient slider.

Once enabled it **overrides all saved preset tempos** for every preset in that song, **also responds to the Tap Tempo footswitch**, and appears in the Preset View lower ribbon and on the Tap scribble strip. Default is **off**.

### Song Notes

Touch **`+ Song Notes`** (centred below the instruction bar) while assigning presets. Short performance reminders — guitar, tuning, capo. They render **in parentheses on a second, smaller line** beneath the song name in **Gig View for both Songs and Setlists** modes.

### Manage

More Options (`⋯`) → **delete, duplicate or rename** a song. Editing a preset from within Songs mode and saving it **overwrites every use of that preset**.

### Navigate

Touch a song title or turn the navigation control to load that song's presets onto the footswitches, then **press a footswitch to actually load a preset**. The two far-left switches become **PREV SONG (top-left)** and **NEXT SONG (bottom-left)**. Songs are listed **alphabetically**.

Performance scribble strips show the section label over `<slot> <PRESET NAME>`; the assignment screen shows label over preset name **without** the slot number.

---

## 7. Setlists (pp. 28–29)

**Create:** Setlists mode → `+` → name → `CONFIRM`. Then `+` to add songs → **select one or more** from the list → `CONFIRM`. Repeat to add more.

Songs in a setlist are **numbered on the left** and reordered by drag-and-drop; `⋯` removes one. Setlists themselves are numbered (`01`…), i.e. **manually ordered, not alphabetical** — unlike songs.

`⋯` on a setlist renames or deletes it. Editing a song from a setlist **affects every use of that song**.

**A setlist must be selected and a footswitch pressed before a new preset loads** — selecting a setlist alone does not change the sound. The Setlists footswitch grid is identical to the Songs one.

**Setlist Gig View:** press the navigation control. Large-font setlist with the current song in blue. **Turning the navigation control scrolls through setlists**; swipe up/down or use the two far-left footswitches to move through **songs**. Exit via the navigation control or the back button beside the setlist name.

---

## 8. Looper (p.30)

Enter by **holding the MODE footswitch for two seconds**. Stereo, up to **three minutes** at full speed, available in every preset, and **playback continues across preset changes**. All looper parameters are **global**.

Footswitch layout while in Looper mode:

|            | col 1           | col 2            | col 3       | col 4     | col 5                     |
| ---------- | --------------- | ---------------- | ----------- | --------- | ------------------------- |
| **top**    | `LOOP VOL UP`   | `UNDO`           | `1/2 SPEED` | `REVERSE` | `EXIT` / `HOLD: POSITION` |
| **bottom** | `LOOP VOL DOWN` | `RECORD OVERDUB` | `PLAY STOP` | `1-SHOT`  | `TAP` / `HOLD: TUNER`     |

- Loop volume steps in **0.5 dB** increments.
- **1/2 Speed** halves record/playback speed — playback drops an octave; recording at half speed then returning to full plays an octave **up**.
- **Undo** removes the last recorded layer.
- **Hold EXIT** moves the looper to the **front** of the instrument signal path (its LED turns **purple**); hold again to return it to the end.
- **Tap tempo and the tuner remain available** inside Looper mode — the TAP/TUNER switch is unchanged.

---

## 9. Tap tempo and tuner (pp. 31–32)

**Tap tempo:** tap the TAP/TUNER footswitch **at least twice**. The LED ring flashes at the current tempo; BPM shows in the scribble strip; the touchscreen shows a BPM gradient **for three seconds** (as an overlay on the right of the live Preset View) with `+`/`−` fine-tune. Only delay and modulation effects with a **`TAP DIV`** parameter assigned follow tap tempo — `TAP DIV` lives in each effect's own edit screen.

**Tuner:** hold TAP/TUNER for **two seconds**, or open it from Global Settings. Details and defaults in `global-settings.md` §7.

---

## 10. DAW Mode (p.45)

Enter by **holding the FS Mode and Tap Tempo footswitches together for two seconds**. Footswitches rebind to **Fender Studio Pro** transport control:

|            | col 1         | col 2      | col 3      | col 4    | col 5                 |
| ---------- | ------------- | ---------- | ---------- | -------- | --------------------- |
| **top**    | `CLICK`       | `PRE-ROLL` | `PUNCH IN` | `LOOP`   | `EXIT`                |
| **bottom** | `RETURN ZERO` | `STOP`     | `PLAY`     | `RECORD` | `TAP` / `HOLD: TUNER` |

`EXIT` takes the MODE-switch position; the TAP/TUNER switch is unchanged. **DAW Mode does not take over the touchscreen** — the ordinary Preset View stays on screen.

> The PDF's text layer for this page carries non-rendered "ghost" strings describing a different,
> track-oriented layout — see `open-questions.md` F1 for the full list and the
> corroboration. Treat the ghost strings as non-authoritative; the **visible** layout above is
> correct.

---

## 11. Backup, restore and firmware

### Backup (p.33)

**Two routes, and the manual gives no equivalence between them:**

1. **micro SD** — Global Settings → Preferences → **Backup presets, settings and IRs to SD card**. Up to **25** backups with custom names. **Restore** is a separate menu item beside it. Card not included.
2. **Tone Master Pro Control desktop app** (Mac/PC, over USB) — does "select, edit, **backup/restore** and share your presets" (p.1).

The manual never enumerates what "settings" covers — in particular whether **Songs and Setlists** are inside a backup. See `open-questions.md`.

### Firmware update (p.44)

**It is a USB mass-storage drag-and-drop, not a protocol transfer.**

1. Download the firmware from `fender.com/tonemaster_pro`.
2. Connect the USB-C cable.
3. **Press and hold the rear-panel update button for 10 seconds while powering the unit on.** (Button location: §1 of `setup-recipes.md`.)
4. The unit displays **"USB Firmware Update Mode"**.
5. **Drag and drop the `.img` file onto the mounted drive named `FENDER_AMP`.**
6. "Applying updates. This may take several minutes. Do not power off until update is complete." → **"Update succeeded. Please restart to continue applying update."** → restart.

Note the device's own wording says the update **continues after the restart**, where the page prose implies the restart merely follows completion.

### Factory reset (p.33)

Global Settings → Preferences → **Factory Reset** — restores **all presets and settings**.

---

## 12. Power-up behaviour (p.6)

- **First ever power-on:** My Presets, preset 1, signal path visible.
- **Every subsequent power-on:** restores the **last used mode, setlist, song and preset**.

Two globals deliberately do **not** persist: **Reamp mode** resets to OFF, and **Global EQ** returns to flat unless `Retain Global EQ` is on.
