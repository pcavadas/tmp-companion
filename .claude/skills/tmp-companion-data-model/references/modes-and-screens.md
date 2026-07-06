# Operating modes, screens, and the block inventory

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

## Block inventory (orientation counts, firmware 1.7)

Don't duplicate the catalog here — it stales on every firmware update. Source precedence is scoped per fact: the **Model Guide wins on official names, real-unit attributions, and appearance**; the firmware's **`product_profile.json` outranks both on on-device availability + menu category** (the guide and app both include unavailable leaks — the `tmp-companion-catalog` skill operationalizes this hierarchy and owns the shipped `tmp-model-guide.json`).

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
| Effects: Reverb      | ~22                      | many **convolution** — see `references/constraints.md`                                                                              |
| Effects: Dynamics    | ~11                      | compressors, gates, volume swell, slow attack                                                                                       |
| Effects: EQ          | ~10                      | 5/7/10-band graphic, 3/5-band parametric, LP/HP/notch filters                                                                       |
| Effects: Filter      | 5                        | 3 wahs + Filtron + Enigma envelope                                                                                                  |
| Effects: Pitch       | 10                       | Micro/Chromatic/Polygon/Polyvoice/Diatonic, Pedal Detune/Shifter, Virtual Capo, Granular Arpeggiator, Feedback Generator            |
| Effects: Synth       | 3                        | Cerberus + Aethon polysynths, Wavemorph                                                                                             |
