# Device facts the manual documents that our code does not model

Derived from a full read of the Tone Master Pro Owner's Manual v1.8 (rev. J) — text layer plus every page rendered as an image — cross-checked against this repo, and closed where possible against a real unit (fw 1.8.45).

**Most of these are checks to run, not confirmed defects.** Each entry states the manual fact, the code that assumes otherwise, and — where still open — a probe.

The device reference itself lives in the **`tmp-companion-data-model`** skill (`references/setup-recipes.md`, `global-settings.md`, `workflows.md`, `open-questions.md`). This file is only the delta list, and entries should be **deleted** as they're closed.

---

## 1. The Output Mixer sits on the measurement tap, and is only readable offline

The leveller and the Doctor both measure **USB 1/2**. Per the manual (p.36) that is a full mixer channel with its own **fader, mute and solo** — plus AUX and Bluetooth injection buttons. None of that lives in preset data.

**Measured (fw 1.8.45):**

- **Readable, but only from a device backup.** `settingsBackup.mixerSaveData` carries the full per-channel strip. The live path (`MixerMessage`, TMS 5) reads as unserved and no write route has been confirmed either. `probe --mixer` implements the read sweep.
- **The USB 1/2 fader (not the master knob) is in the measurement path, and routing is per-preset** — full figures and the per-preset-recheck requirement are in `notes/gotchas.md#output-assign-is-per-preset-and-is-applied-to-the-global-mixer-on-every-preset-load`.
- **Mute, solo-elsewhere and AUX** all measured: muting or having another channel soloed (with `usb12` itself not soloed) both produce `no signal captured`; also-soloing `usb12` restores the reading (solo is additive, p.36); AUX with nothing physically connected is silent. **Solo-elsewhere does NOT flip `usb12`'s own `soloActive` flag** — a pre-flight must check "is any _other_ channel's `soloActive` true", not just `usb12`'s own booleans. BT injection is untested but schema-identical to AUX.

Failure modes still undetectable at runtime:

- **Fader not at unity** → every capture carries a constant offset; solve-then-verify still reports ~0.00 LU error because both measurements pass through the same attenuation.
- **USB 1/2 muted, or another channel soloed** → deep silence, currently attributed solely to a dropped `set_reamp_mode(true)`.
- **AUX or Bluetooth routed into USB 1/2** → external audio contaminates the capture and could lift it past the floor-trip guard.

**Not yet built:** the app already runs a backup scan at startup (`libraryScan`) and `read_backup_archive` already parses this archive — `settingsBackup` is simply not decoded yet. Warn when `usb12` is muted, when another channel is soloed and `usb12` is not, when AUX/BT is injected into `usb12`, or when `usb12.faderLevel != 1.0`. Each is a silent-corruption or silent-silence cause today.

---

## 2. `Scene Change Behavior: DISCARD CHANGES` breaks batched scene leveling

`save_deferred_scene_writes` deliberately accumulates **unsaved** scene writes across scene recalls and saves once at batch end. The manual (p.35) states that under `DISCARD CHANGES`, switching scenes without saving **loses the edits**. `probe --defer-scenes` validated the deferred-write behaviour only on a unit at the `MAINTAIN CHANGES` default.

**Measured (fw 1.8.45):** the setting is readable (`settingsBackup.sceneChangeBehavior`, `0 = Retain / MAINTAIN CHANGES`, `1 = Revert / DISCARD CHANGES`) off the same startup backup scan already in gap 1. The underlying mechanism is confirmed directly on the touchscreen: with `DISCARD CHANGES` set, an unsaved manual edit (Master Vol) and an unsaved FS-toggled bypass are both reverted the moment their scene is recalled again (a same-scene footswitch re-press counts as a recall). No live write route to this setting has been confirmed — a touchscreen edit is the only confirmed way to reach `DISCARD CHANGES` today.

**Guarded (2026-08-30):** `scene_discard_guard` (`commands/level_scenes.rs`, decode in `backup_read::scene_change_behavior`) refuses every deferred-write lane — `level_scenes_apply_batched`, `level_footswitches_apply`, `redistribute_headroom`, and `restore_redistribution` — before any device write when the snapshot reads `DISCARD CHANGES`, naming the touchscreen fix (Settings → Scene Change Behavior → `MAINTAIN CHANGES`) and the replug that refreshes the snapshot. The stale-snapshot asymmetry mirrors gap 4's #124 fader handling: only a positively-read `DISCARD` refuses (the failure it causes is silent corruption of an unrevertable save, and a wrongly-refused stale read is cleared by the same replug the message names); an absent/unreadable snapshot or unknown ordinal proceeds, because the factory default is `MAINTAIN` and blocking every fresh install on a missing snapshot punishes the common case on no evidence. Refuse rather than warn — the app is click-only for non-technical users, and a click-through warning recreates the corruption. Gate: `scene_discard_guard_tests` (`level_scenes.rs`).

**Still open:** whether `save_deferred_scene_writes`'s own code path re-recalls each scene before its final save (which would trip this mechanism) is not established — that depends on whether its read-back goes through a `loadScene`-style device recall or a passive read. The answer decides whether `DISCARD CHANGES` could ever be _supported_ rather than refused; until it's checked against a live unit, the guard stands.

---

## 3. Mic/Line-only presets get no stimulus

`audio.rs` injects only into `REAMP_INSTRUMENT_OUT_CH = 2` (USB-In 3 = **instrument** channel). Re-amp USB-In **4** feeds the mic/line channel and we never drive it.

Three of the twelve templates are mic-only — `Mic/Line Series`, `Mic/Line Parallel 1`, `Mic/Line Split`. A preset on any of them receives silence, producing `"no signal captured"` / off-branch. The honest diagnosis is "this preset's path is mic/line and we only drive instrument", not a device fault.

The template is knowable **offline** from the backup scan (`micNodes` vs `guitarNodes` in `audiograph.rs`), so this can be detected and reported without a device read.

---

## 4. USB 3/4 Pre/Post scales the Tier-2 DI capture

Tier-2 calibration captures the dry instrument from `DRY_INSTRUMENT_IN_CH = 2` (USB-Out 3) and stores its K-weighted loudness as `calibration_lufs`. That send has a **Pre/Post** control, and the p.36 screenshot shows **USB 3 and USB 4 each have their own independent pair** (the prose describes it as one choice for "USB 3/4"). Default is `PRE` on both. The channel also has its own fader and mute.

- `PRE` — fader-independent, sent at a 0 dBFS reference.
- `POST` — fader-scaled. A user on `POST` with the USB 3 fader down records a quieter DI, giving a `calibration_lufs` that under-drives every later re-amp.

Worth stating the `PRE` assumption explicitly in `notes/leveling.md`, and worth checking first the next time a calibration reads implausibly quiet.

**Tracked as [#124](https://github.com/pcavadas/tmp-companion/issues/124) — pre-flight LANDED (2026-08-22), read-only.** A real unit on fw 1.8.58 had BOTH `usb3` and `usb4` strips `muteActive: true` in its `settingsBackup.mixerSaveData` — every calibration take read `[USB-Out 1/2 -14 dBFS · 3 silent · 4 silent]`, i.e. the guitar processed but no dry send at all. `calibrate_profile` now decodes the strip (`backup_read::usb3_strip`) from the settings snapshot the startup backup read persists to `support/device-settings.json`: the snapshot can be stale (the mixer may have been touched since connecting), so the two halves treat it ASYMMETRICALLY, and the asymmetry is deliberate:

- **Mute only EXPLAINS a silent take.** It is consulted on the silent-capture path alone, where it outranks the lane-peak guesses (with the on-unit fix: Mixer → USB 3 → unmute). A take that lands despite a "muted" snapshot is simply a newer mixer state, and wins. It is deliberately not attached to other capture failures — doing so once turned a mid-capture unplug into a confident "USB 3 is MUTED".
- **`POST` with an off-unity fader DOES veto a take that landed.** This is the one place a stale snapshot can block real work, and it is the right trade: a successful take persists the capture as well as the scalar, and `resolve_stimulus_for_leveling` injects that WAV **verbatim at gain 1** as the profile's leveling stimulus. A fader-scaled capture would therefore drive every later re-amp through a nonlinear amp chain at the wrong level — wrong breakup, wrong `C`, silently wrong leveling and Doctor verdicts until someone recalibrates. Refused rather than compensated, because the fader's dB law is unmeasured. Recovery is in the message: fix the strip on the unit, then replug (a detach fires `resetLibraryScan`, so the next connection re-reads the mixer).

**A second unit shows a clean strip exists — whether the mute is a fw 1.8.58 default is unresolved.** The development unit (**fw 1.8.45**, `probe --capture-input 6` / `--fw`, 2026-08-24) reports `usb3`/`usb4` both `muteActive: false`, `preEnabled: true`, `faderLevel: 1.0` — the clean state — and negotiates the full 4-channel capture with an actual dry lane on USB-Out 3. Whether the reporting unit's muted USB 3/4 is a fw 1.8.58 DEFAULT or that unit's own mixer history is UNRESOLVED: two units on two firmwares cannot separate the two, and only a fresh or factory-reset fw 1.8.58 unit can. What the second unit does establish is that a clean strip exists in the field, so do not "fix" this by assuming new firmware mutes the dry sends — the app keeps diagnosing the strip per unit rather than per firmware.

**Floor caveat, same run (fw 1.8.45) — and why the silence guard is no longer a peak threshold.** That unit's idle dry lane peaks at −74.6 dBFS (ch0/ch1 at −71.5), i.e. ABOVE the −80 dBFS peak floor the guard first used. A not-playing take there was therefore never classed as silent: it PASSED, and surfaced much later as the terser "captured signal too quiet to measure" from the non-finite-LUFS gate, so the richer diagnosis never ran at all (confirmed: the probe reads `lufs -inf` on all four lanes).

Retuning the constant would have been the wrong fix, and these same two numbers say why. A floor chosen to clear this unit's −74.6 dBFS is a guess about the next unit's; worse, ch0/ch1's −71.5 dBFS ALSO clears −80, so simply moving the peak test to a later gate would have left the lane hint reading an idle processed pair as signal and telling a user on this spotless unit that USB 3 was MUTED. The guard therefore keys on integrated loudness per lane (`DryLane::measurable`, `probe_api/stimulus.rs`) — BS.1770's own −70 LUFS ABSOLUTE gate — which needs no tunable and no per-unit floor survey: any floor under that gate is `-inf` LUFS, and both measured floors sit well under it (a floor loud enough to integrate above −70 LUFS is signal to any silence gate, and the downstream active-window and spread checks own that case). The lane READOUT still prints peaks, and prints an unmeasurable lane as `silent` rather than as its floor's dBFS. Gate: `an_idle_lane_above_the_old_peak_floor_still_faults_and_blames_no_one`, fixtured on the two peaks above.

Still open: a confirmed live mixer WRITE (none exists — see `open-questions.md` A1) would let the app unmute the strip itself.

_(Note: the manual's "at 0dBfs (maximum level)" describes the send's full-scale reference for an unattenuated, fader-bypassed signal. It does not document the absence of a limiter — that remains our own HW observation, not a manual-derived fact.)_

---

## 5. Re-amp skips analog loops 1/2

p.37 is explicit: incoming USB audio is not routed through loops 1 or 2, which are analog and sit before the A/D. A preset whose tone depends on an external fuzz or drive in loop 1/2 therefore **cannot be measured or diagnosed faithfully** — part of its chain is absent from the capture.

The loop block is visible in the graph, so this is detectable offline and could be surfaced as a per-preset caveat in the Level summary and the Doctor, rather than reporting a confident number for a chain that was never fully in the path.

---

## 6. Preset Spillover — tail duration measured; a separate capture-pipeline residual found and ruled out as NOT spillover

p.17 documents a per-preset on/off for hearing delay and reverb tails when changing presets. Spillover is a documented mechanism for the **previous** preset's tail to leak into a capture taken shortly after `load_preset` — and because it is **per-preset**, it can vary row to row within one batch.

**Measured (fw 1.8.45):**

- `audioGraph.spillover` is readable offline per preset from the backup scan — surveyed at `true` on 38/38 non-empty presets on this unit (uniform today, though the field could vary in a mixed library).
- **A real wet preset's tail runs SECONDS, not milliseconds.** A 0.5 s white-noise burst through a Reverb scene decayed from −46.2 dBFS to −91.8 dBFS over ~4 s (~11.5 dB/s, not yet at the noise floor) — dwarfing the ~1.3–2.5 s inter-connection floor (`SETTLE_AFTER_LOAD_MS` + `RECONNECT_GAP_MS` + a handshake + `SETTLE_AFTER_REAMP_MS`) this repo's own capture shape imposes between a `loadPreset` and its first sample.
- The setting's own on/off toggle is scoped by the manual to **preset changes only** ("hearing delay/reverb tails when changing presets") — scenes are never mentioned. That settles the setting's scope; it does not prove audio can't bleed across a scene recall specifically, which remains untested.
- **A separate, fixed residual exists immediately after any cross-preset load** (~−97 to −116 dBFS, measured identically regardless of what precedes the load — a wet scene, a dry scene, or an unrelated preset). This is a property of this repo's own capture pipeline right after a reload, **not** wet-tail spillover surviving the reload. Its cause (load/reconnect transient, gain-staging settle, ADC self-noise) is unmeasured, but it's real and worth a caveat for anything measuring near-silence immediately after a preset change in this repo (mute/solo null checks, a quiet preset following any other, any future "no signal" assertion).

**Still open:** audio bleed during a scene recall specifically (not a preset change), and the residual's own mechanism. Not yet measured: an A/B with spillover toggled OFF on a real preset (a per-preset touchscreen edit).

---

## 7. 48 kHz is a USB rate, not the device's internal rate

`leveller.rs` hardcodes `RATE = 48_000`. The spec sheet (p.46) lists the internal A/D–D/A at **44.1 kHz** and the USB audio clock as DAW-selectable 44.1/48/88.2/96. The precise claim is _"the macOS Core Audio device must be set to 48 kHz"_ — not _"the device clock is 48 kHz"_. Measured and confirmed — see `notes/gotchas.md#48-khz-stimulus-required` for the spectral evidence.

Still open:

- Anything reasoning about capture content above ~22 kHz (spectrum reports, the Doctor's fine-PSD `peaks`, EQ-match) is reading the anti-alias skirt, not preset tone. Worth an upper bound on analysed bandwidth.
- The device carries a global `sampleRate` setting (`settingsBackup.sampleRate`, `0` here). Whether the band limit moves with it is untested — do not assume 22 kHz is fixed.

---

## 8. Leveling a scene flips its amp block to Scene-Edit-enabled

The code calls `set_node_scene_edit(group, node, true)` before every per-scene write — correct, and it is what isolates the write to that scene.

But note the authoring change: if the user had deliberately left that amp block **Scene Edit off** (sharing one amp setting across all scenes, p.21), leveling silently turns it on, and from then on the block's parameters no longer sync across scenes.

The manual's asymmetry means the current direction is the safe one — **enabling** is the isolation mechanism, whereas **disabling** reverts the block's parameters to the base preset. So there is no cleanup to add, and specifically **do not** try to "restore" the flag by disabling it afterwards. Worth one line in the summary.

---

## 9. `moveUserPreset` verification must use a fresh connection

The firmware itself rewrites Song→preset and setlist→song bindings to follow a reordered preset or renumbered song — reordering is safe at the device level (see `open-questions.md` D1/D2). The trap is in our own code.

`session.move_user_preset` is **fire-and-forget and emits no `presetError`**, and a `list_my_presets` issued on the **same connection** afterwards serves the **pre-move** state — so a same-connection verify reports "the reorder did not land" for a reorder that landed.

Anything that reorders presets and then verifies (or reports success to the user) must **drop the connection and reconnect** before reading back. `commands/presets.rs`'s `move_preset` command has no read-back at all today, so a dropped reorder would surface as nothing.

**MIDI PC mappings** (preset addressed by bank + program number) were **not** tested and remain unknown.

---

## 10. `--seed-scenario` re-import intermittently reads a truncated preset list

`replace_inplace_with`'s own `Session::connect()?.list_my_presets()?` read has returned as few as 321–372 of the expected 504 entries, always failing to find list index 400, right after `--clear`-ing scratch slots 400/401/402. An independent, immediately-following bare `probe` call reads all 504 entries cleanly, so the device itself is not degraded — this is specific to the read this function issues in this sequence. The existing "tolerant reads tail-truncate safely" design holds (no target slot is ever misidentified or clobbered; every attempt aborts before any write), but the truncation itself has not cleared with waits up to ~45 s or repeated retries.

**Not yet root-caused** — candidates include residual connection state from the preceding `--clear` calls that this function's fresh connect doesn't fully wait out, or a device-side backlog effect after many prior connects in one session. Workaround: author the scratch preset directly on the touchscreen instead of via the fixture importer.

---

## 11. Smaller notes

- **Song footswitches can hold a Scene, not just a preset** (p.27). Confirm the decoded `SongPresets` map carries scene addressing rather than flattening to the base preset.
- **Preset Volume is Fender's own name for what this app automates** — p.17 describes it as "for normalizing volume of all presets", 0–100 %. Good framing for user-facing copy.
- **The global −6 dB instrument pad** is on the analog input, so it shifts a Tier-2 DI capture but not a re-amped USB capture — an asymmetry between the two calibration paths.
- **Loop 3 = LEFT, Loop 4 = RIGHT** when configured as one stereo loop (silkscreened on the chassis, absent from the prose).
- **MIDI bank 3 (0-based: CC0 value 3, the 4th and last bank) holds 120 presets (program numbers 1–120), not 128** — banks 0–2 hold 128 each. Relevant to any MIDI-recall feature; see `SKILL.md`'s MIDI implementation section for the full addressing table.
- **"Bank" is overloaded**: six presets per footswitch bank (pp.5, 9) versus 128 presets per MIDI bank (p.42).
- **Splitter and mixer render with an identical glyph** (p.12, p.18) — position in the chain is the only differentiator, so `SignalChainView` must not key on glyph identity.
