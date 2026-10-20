# On-device operation — songs, setlists, modes and maintenance

Source: Owner's Manual v1.8.
Page refs are **printed** pages.
Device facts tagged here are for firmware 1.8.58 unless a line says otherwise.

`workflows.md` covers editing a preset (presets, blocks, cabinets, footswitches, EXP); this file covers playing and maintaining the unit, and indexes every screen.

---

## 1. Songs (pp. 26–27)

### Create

Songs mode → **`+`** at upper right → name via keyboard → `CONFIRM`.
The **Song Footswitch Assignment** screen appears: a 5 × 2 grid where **columns 2–4 of both rows are the six assignable slots** (column 1 is PREV/NEXT SONG, column 5 top is the MODE switch).

**Assign a preset or scene:** touch a slot showing `+` → the **Add Preset to Song** screen (searchable) → `CONFIRM`.
A **Scene** can be assigned, not just a whole preset.

**Shortcut:** hold an **empty** song footswitch for **two seconds** to jump to Add Preset To Song.

Each assignment's editor has exactly three rows: `Preset` · `Label` · `Color (Active/Inactive)` (default **`DEFAULT/DEFAULT`**).
Labels come pre-written for song sections — intro, verse 1, chorus 1, solo, verse 2, outro.
These colours **override the global default preset footswitch colours**.

**Reorder:** drag-and-drop a slot; if the destination is occupied the two **swap**.

### Song BPM — "how do I set the tempo of a song"

The **Song BPM** control occupies the **bottom-right cell** of the assignment grid and reads `SONG BPM` over `OFF` when disabled.

1. **Touch the virtual footswitch** to enable Song BPM.
2. **Touch the virtual scribble strip** to set the value with a gradient slider.

Once enabled it **overrides all saved preset tempos** for every preset in that song, **also responds to the Tap Tempo footswitch**, and appears in the Preset View lower ribbon and on the Tap scribble strip.
A tap writes the song's BPM only while Song BPM is on, and turning Song BPM on re-applies the stored BPM [HW+RE].
Default is **off**.

### Song Notes

Touch **`+ Song Notes`** (centred below the instruction bar) while assigning presets.
Short performance reminders — guitar, tuning, capo.
They render **in parentheses on a second, smaller line** beneath the song name in **Gig View for both Songs and Setlists** modes.

### Manage

More Options (`⋯`) → **delete, duplicate or rename** a song.
Deleting a song also removes its preset bindings and every setlist membership, and later songs shift down by one [RE].
Editing a preset from within Songs mode and saving it **overwrites every use of that preset**.

### Navigate

Touch a song title or turn the navigation control to load that song's presets onto the footswitches, then **press a footswitch to actually load a preset**.
The two far-left switches become **PREV SONG (top-left)** and **NEXT SONG (bottom-left)**.
Songs are listed **alphabetically**.

Performance scribble strips show the section label over `<slot> <PRESET NAME>`; the assignment screen shows label over preset name **without** the slot number.

---

## 2. Setlists (pp. 28–29)

**Create:** Setlists mode → `+` → name → `CONFIRM`.
Then `+` to add songs → **select one or more** from the list → `CONFIRM`.
Repeat to add more.

Songs in a setlist are **numbered on the left** and reordered by drag-and-drop; `⋯` removes one.
Setlists themselves are numbered (`01`…), i.e. **manually ordered, not alphabetical** — unlike songs.

`⋯` on a setlist renames or deletes it.
Editing a song from a setlist **affects every use of that song**.

**A setlist must be selected and a footswitch pressed before a new preset loads** — selecting a setlist alone does not change the sound.
The Setlists footswitch grid is identical to the Songs one.

**Setlist Gig View:** press the navigation control.
Large-font setlist with the current song in blue.
**Turning the navigation control scrolls through setlists**; swipe up/down or use the two far-left footswitches to move through **songs**.
Exit via the navigation control or the back button beside the setlist name.

---

## 3. Looper (p.30)

Enter by **holding the MODE footswitch for two seconds**.
Stereo, up to **three minutes** at full speed, available in every preset, and **playback continues across preset changes**.
All looper parameters are **global**.

Footswitch layout while in Looper mode:

|  | col 1 | col 2 | col 3 | col 4 | col 5 |
|---|---|---|---|---|---|
| **top** | `LOOP VOL UP` | `UNDO` | `1/2 SPEED` | `REVERSE` | `EXIT` / `HOLD: POSITION` |
| **bottom** | `LOOP VOL DOWN` | `RECORD OVERDUB` | `PLAY STOP` | `1-SHOT` | `TAP` / `HOLD: TUNER` |

- Loop volume steps in **0.5 dB** increments.
- **1/2 Speed** halves record/playback speed — playback drops an octave; recording at half speed then returning to full plays an octave **up**.
- **Undo** removes the last recorded layer.
- **Hold EXIT** moves the looper to the **front** of the instrument signal path (its LED turns **purple**); hold again to return it to the end.
- **Tap tempo and the tuner remain available** inside Looper mode — the TAP/TUNER switch is unchanged.

---

## 4. Tap tempo and tuner (pp. 31–32)

**Tap tempo:** tap the TAP/TUNER footswitch **at least twice**.
The LED ring flashes at the current tempo; BPM shows in the scribble strip; the touchscreen shows a BPM gradient **for three seconds** (as an overlay on the right of the live Preset View) with `+`/`−` fine-tune.
Only delay and modulation effects with a **`TAP DIV`** parameter assigned follow tap tempo — `TAP DIV` lives in each effect's own edit screen.

**Tuner:** hold TAP/TUNER for **two seconds**, or open it from Global Settings.
Details and defaults in `global-settings.md` §7.

---

## 5. DAW Mode (p.45)

Enter by **holding the FS Mode and Tap Tempo footswitches together for two seconds**.
Footswitches rebind to **Fender Studio Pro** transport control:

|  | col 1 | col 2 | col 3 | col 4 | col 5 |
|---|---|---|---|---|---|
| **top** | `CLICK` | `PRE-ROLL` | `PUNCH IN` | `LOOP` | `EXIT` |
| **bottom** | `RETURN ZERO` | `STOP` | `PLAY` | `RECORD` | `TAP` / `HOLD: TUNER` |

`EXIT` takes the MODE-switch position; the TAP/TUNER switch is unchanged.
**DAW Mode does not take over the touchscreen** — the ordinary Preset View stays on screen.

> The PDF's text layer for this page carries non-rendered "ghost" strings describing a different, track-oriented layout — see `manual-errata.md` F1 for the full list and the corroboration.
> Treat the ghost strings as non-authoritative; the **visible** layout above is correct.

---

## 6. Backup, restore and firmware

### Backup (p.33)

**Two routes, one archive:**

1. **micro SD** — Global Settings → Preferences → **Backup presets, settings and IRs to SD card**.
   Up to **25** backups with custom names.
   **Restore** is a separate menu item beside it.
   Card not included.
2. **Tone Master Pro Control desktop app** (Mac/PC, over USB) — does "select, edit, **backup/restore** and share your presets" (p.1).

The manual never enumerates what "settings" covers.
Both routes build the same archive: the whole user database (presets, **Songs, Setlists** and setlist membership), the global settings JSON, the user IR list and IR files, and the block-preset defaults [RE].

### Firmware update (p.44)

**It is a USB mass-storage drag-and-drop, not a protocol transfer.**

1. Download the firmware from `fender.com/tonemaster_pro`.
2. Connect the USB-C cable.
3. **Press and hold the rear-panel update button for 10 seconds while powering the unit on** (button location: §1 of `setup-recipes.md`).
4. The unit displays **"USB Firmware Update Mode"**.
5. **Drag and drop the `.img` file onto the mounted drive named `FENDER_AMP`.**
6. The unit shows "Applying updates. This may take several minutes. Do not power off until update is complete."
   It then shows **"Update succeeded. Please restart to continue applying update."**; restart it.

Note the device's own wording says the update **continues after the restart**, where the page prose implies the restart merely follows completion.

### Factory reset (p.33)

Global Settings → Preferences → **Factory Reset** — restores **all presets and settings**.

---

## 7. Power-up behaviour (p.6)

- **First ever power-on:** My Presets, preset 1, signal path visible.
- **Every subsequent power-on:** restores the **last used mode, setlist, song and preset**.

Two globals deliberately do **not** persist: **Reamp mode** resets to OFF, and **Global EQ** returns to flat unless `Retain Global EQ` is on.

---

## Screen index

The firmware exposes the following screens / modals.
Each is a _product surface_ — a defined view the user can be inside.

| Surface | Purpose |
|---|---|
| `List View` | scrollable preset/song/setlist list (any of the 6 operating modes) |
| `Preset View` | current preset's signal chain, upper ribbon (List View / star / number box / Save / gear), lower ribbon (EXP Assign / Footswitch Assign / Preset Settings / Add Block / Tap) |
| `Gig View` | fullscreen preset name + number (preset mode) OR song list (Songs/Setlists mode) — minimal, accident-resistant performance view |
| `Block Edit` | zoomed-in view of one block with 6 visible parameters + PAGE footswitch for additional pages |
| `Cabinet Settings` | 32-position mic grid + cab/mic selectors + axis/filters + dual-cab/dual-mic Blend+Pan + External Cabinet + SIC |
| `Add Block` menu | category list + model list with audition + Block Preset expand |
| `Move/Delete` | block reorder mode (long-press triggered), drag-to-bottom to remove |
| `Footswitch Assign` | 8-footswitch panel for editing Effects FS mode assignments |
| `EXP Assign` | 5-source panel for editing pedal/toe/MIDI-EXP parameter targets |
| `Preset Settings menu` | Preset Volume / Signal Path Type / Input Impedance / Output Assign / Preset MIDI / Preset Spillover / Amp Control |
| `Save dialog` | name field + Save Location list + "select next empty preset" shortcut |
| `Global Settings` | gear-accessed, with 7 bottom tabs: Preferences / I/O / Footswitch / Bluetooth / EQ / Mixer / Tuner |
| `Tuner` | full-screen chromatic tuner with reference frequency + mute toggle + INSTRUMENT/MIC-LINE selector |
| `Mixer` | per-output faders (Headphones / OUT 1 / OUT 2 / USB 1-2 / USB 3 / USB 4) with AUX, Bluetooth, Mute, Solo, PRE/POST |
| `Looper` | modal — looper transport footswitch layout (hold-2s entry) |
| `DAW Mode` | modal — Fender Studio Pro transport footswitch layout (hold-2s entry) |
