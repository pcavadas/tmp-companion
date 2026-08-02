# Danger rules — data loss, device wedging, machine crashes

**This file has no `paths:` frontmatter on purpose.** It loads unconditionally and is re-injected after compaction, exactly like `CLAUDE.md`. Every rule here was learned by running a real Tone Master Pro; each one is expensive or impossible to relearn, and none is visible in the code.

Full hardware evidence for the linked entries is in [`notes/gotchas.md`](../../notes/gotchas.md) — read the entry before changing the behaviour it governs.

---

## Destructive writes

- **DANGER** — **Slot addressing: device `userSlot` = list index + 1** (HW-confirmed 1.7.75). [→ evidence](../../notes/gotchas.md#slot-addressing-device-userslot--list-index--1)

- **DANGER** — **Confirm a slot mapping with a non-destructive READ first, and put the guard in the SAME ADDRESS SPACE as the mutation.** Before any destructive op (`clear` / `move` / `save`-over) keyed on a slot/index mapping. A `clear` once **deleted a real preset** because the guard checked list-index space while `clear` acted in 1-based device-slot space. On real hardware with irreplaceable presets, an unconfirmed mapping plus a wrong-space guard equals silent data loss.

- **DANGER** — **Saving permanently alters a preset.** Opt-in via the "Save leveled value to preset" checkbox, which is the only gate. Revert/backup of the original `presetLevel` is still TODO, so a save cannot be undone from the app.

## Re-amp and measurement

- **DANGER** — **NEVER re-engage re-amp on a held connection** (disengage → settle → re-engage). HW-observed to wedge the device's re-amp **and trigger a USB crash that rebooted the Mac**. `leveller::level_preset` therefore uses **three fresh connections** (load / measure / apply). For SCENE and footswitch leveling only the measurement prepass reconnects (one engage per scene); that path's apply — per-scene `outputLevel` + save, all pure SENDS with no engage — runs on ONE persistent session. Either way the run must still end with a guaranteed re-amp OFF on its own fresh connection. See `notes/protocol.md`.

- **DANGER** — **A silent/failed re-amp inject reads as the device's STATIONARY OUTPUT FLOOR**, and `measure_c` would accept it as a valid `C` without the production floor guards. In a rapid 20-engage `probe --stim-ab` sweep, **19 of 20** captures measured the post-DSP floor rather than the stimulus. [→ evidence](../../notes/gotchas.md#a-silentfailed-re-amp-inject-reads-as-the-devices-stationary-output-floor-and-measure_c-would-accept-it-as-a-valid-c-without-the-production-floor-guards-below)

- **Re-amp engages reliably only ONCE per connection.** Fresh-connect per engage. The `ReAmpModeChanged` echo is flaky and is NOT proof of engagement — a finite captured loudness is. [→ evidence](../../notes/gotchas.md#re-amp-engages-reliably-only-once-per-connection)

- **Re-amp latches preset state at engage** — the captured tap reflects only the `presetLevel` set BEFORE engaging. Set level, then engage.

- **Latch nuances (fw 1.8.45).** `changeParameter` IS audible mid-engage — live knob nudges work, and the whole live-leveling family rests on this. But `loadScene` mid-engage is INAUDIBLE: the active scene latches at engage (all 9 scene rows of an 8-scene preset measured identical audio on one engage), so per-scene leveling requires one engage per scene. Separately, `load_preset` + engage in the SAME connection captures pure silence — load in its own connection, then fresh-connect to set and engage (the `measure_knob_at` shape). _Keep these as distinct facts: `presetLevel` is a load-time latch, `changeParameter` is a mid-session live write._

- **Re-amp toggle** is `SettingsMessage(3) → reampModeActive(30)`, NOT MixerMessage. ON = `1a05f201020801`, OFF = `1a03f20100`.

- **Leveling runs must end with a GUARANTEED re-amp OFF on a fresh connection.** A dropped OFF strands the unit input-muted. Recovery: `cargo run --bin probe -- --reamp-off`. Cancel never early-returns past the re-amp engage for this reason, and the post-cancel restore/deferred-save cleanup always runs to completion.

- **`load_preset` + `set_preset_level` in the SAME connection → the set is overridden** by the load's own level-apply. Load in its own connection; a no-load set on the already-current preset sticks and persists across USB reconnects.

- **The scene-context rule: a bare write/measure with no preceding `loadScene` lands in whatever scene the connection currently holds, never base by default.** A preset loads into its SAVED `lastLoadedScene`. [→ evidence](../../notes/gotchas.md#the-scene-context-rule-a-bare-writemeasure-with-no-preceding-loadscene-lands-in-whatever-scene-the-connection-currently-holds-never-base-by-default)

- **OPEN — do not trust scene-0 leveling until resolved.** On the 2-amp Guitar preset, USB `loadScene(0)` materializes a different amp state than the physical footswitch tap.

- **`outputLevel`=0 is DEEP DIGITAL SILENCE on the real TMP**, and `leveller::loudest_loudness` ERRORS ("no signal captured") on a silent capture — a finite LUFS is not recoverable from silence. [→ evidence](../../notes/gotchas.md#outputlevel0-is-deep-digital-silence-on-the-real-tmp-and-levellerloudest_loudness-errors-no-signal-captured-on-a-silent-capture)

- **48 kHz stimulus required** — that is the **host Core Audio rate** the device must be set to, not "the device clock". [→ evidence](../../notes/gotchas.md#48-khz-stimulus-required)

## Resources and connections

- **DANGER** — **`audio::LiveReamp` is ring-buffered (`LIVE_RING_SECS`).** Its capture buffer once grew unboundedly (multi-channel 48 kHz × minutes × dozens of rows) and **OOM'd the whole Mac**. Never reintroduce unbounded capture accumulation; reuse ONE stream pair per preset rather than rebuilding per scene, which also avoids coreaudiod churn.

- **DANGER** — **HID open-lockout model (HW-isolated, fw 1.8.45).** After a session closes the device accepts a QUICK re-open (≤ ~800 ms), then LOCKS OUT exclusive opens (`0xe00002c5`) for tens of seconds — and **every failed open attempt appears to RESET the lockout**. Hammering retries NEVER recovered across hundreds of HW attempts; only a long quiet did. [→ evidence](../../notes/gotchas.md#hid-open-lockout-model-hw-isolated-fw-1845)

- **Exclusive HID seize blocks Pro Control.** `IOHIDDeviceOpen` fails while Pro Control is running; the app surfaces a "close Pro Control" error.

## UI values

- **DANGER** — **`Pick`/`BlockPick` DISPLAY `options[0]` when `value` isn't in `options`** (`options.find(...) ?? options[0]`) — the UI shows one thing while the submitted value is another. Derive defaults from the live option source, never hard-coded ids: a store without a Crunch target displayed "Rhythm" while the run got `"crunch"`, a silent −30 fallback.
