# Danger rules — data loss, device wedging, machine crashes

**This file has no `paths:` frontmatter on purpose.** It loads unconditionally and is re-injected after compaction, exactly like `CLAUDE.md`. Every rule here was learned by running a real Tone Master Pro; each one is expensive or impossible to relearn, and none is visible in the code.

Full hardware evidence for the linked entries is in [`notes/gotchas.md`](../../notes/gotchas.md) — read the entry before changing the behaviour it governs.

---

## Destructive writes

- **DANGER** — **Slot addressing: device `userSlot` = list index + 1** (HW-confirmed 1.7.75). [→ evidence](../../notes/gotchas.md#slot-addressing-device-userslot--list-index--1)

- **DANGER** — **Confirm a slot mapping with a non-destructive READ first, and put the guard in the SAME ADDRESS SPACE as the mutation.** Before any destructive op (`clear` / `move` / `save`-over) keyed on a slot/index mapping. A `clear` once **deleted a real preset** because the guard checked list-index space while `clear` acted in 1-based device-slot space. On real hardware with irreplaceable presets, an unconfirmed mapping plus a wrong-space guard equals silent data loss.

- **DANGER** — **Saving permanently alters a preset.** Opt-in via the "Save leveled value to preset" checkbox, which is the only gate. Revert/backup of the original `presetLevel` is still TODO, so a save cannot be undone from the app.

- **DANGER** — **`saveCurrentPreset` is durable when it returns, but a same-slot `loadPreset` right after a save is a NO-OP** (fw 1.8.58, tmp-audit Q34/Q4): it keeps the live working copy and DSP instead of re-reading the stored row, so it can never "refresh from the device". An edit since the save sets the dirty flag and makes the same-slot load a REAL reload (~0.3 s); to materialize the stored row otherwise, load another slot, then the target. There is no commit window: fw 1.8.45's T+45–100 s lazy commit (the 2026-08-02 preset-24 corruption) does not reproduce, and its barrier is deleted. **NEVER add a preset load before `save_deferred_scene_writes`** — it performs no load itself and saves the run's UNSAVED deferred working-copy writes; those writes set the dirty flag, so a load ahead of it is a real reload that WIPES them. [→ evidence](../../notes/gotchas.md#savecurrentpreset-is-durable-when-it-returns-a-same-slot-loadpreset-right-after-it-is-a-no-op-fw-1858)

- **DANGER** — **A `func: "param"` footswitch with no `valueType` makes the firmware REJECT the whole IMPORTED preset at its first real LOAD** — the rejected-at-load class below, with `presetError` 6 (fw 1.8.58, tmp-audit Q35; HW bisect on fw 1.8.45, 2026-08-09). Every emitted param-func footswitch entry must carry a numeric `valueType`. [→ evidence](../../notes/gotchas.md#a-param-func-footswitch-without-valuetype-makes-the-firmware-silently-replace-the-whole-imported-preset)

- **A footswitch ROW carrying TWO entries (on-off + param on one switch) is LEGAL on fw 1.8.58** (tmp-audit Q35: it loads intact; factory presets carry such rows). On fw 1.8.45 it gutted the whole imported preset to an EMPTY body under the imported name (HW bisect, 2026-08-18). `footswitch.rs`'s `validate_import_body` and `fixture_gates` still refuse it — a conservative choice; relax them only with a HW-checked emitter change. [→ evidence](../../notes/gotchas.md#a-dual-entry-footswitch-row-is-legal-on-fw-1858--it-gutted-imports-on-fw-1845)

- **DANGER** — **A body the firmware rejects at LOAD becomes a silent EMPTY preset that keeps the stored name** (fw 1.8.58, tmp-audit Q35: substituted at the first real load, never later — template `preset_id` `af08d1ac-…`, 0 blocks — and the load answers `presetError` 6; the stored row keeps the imported body). Slot+name confirms pass on it, so a load → save of an imported body can overwrite the original with nothing. `replace_inplace_with` therefore proves the loaded working copy holds the file's blocks (`Session::confirm_loaded_body`) before every save; any new import → load → save path must call it too. [→ evidence](../../notes/gotchas.md#a-body-rejected-at-load-becomes-a-silent-empty-preset-that-keeps-its-name)

- **DANGER** — **An `insertNode` whose anchor is not a node id in its group, or that goes over the CPU budget, gets NO reply and ABORTS the device server** (fw 1.8.58 static RE, not HW-triggered; systemd restarts it and a third crash in 60 s leaves it down). The anchor (field 2) is matched against the group's current node ids, and a replace or insert mints a new one (the model's default id, then `_1`, `_2`…), so never send a FenderId as an anchor or reuse a pre-edit id after a replace: resolve it through `copy_apply`'s `DeviceIds`, and check every insert with `blockcaps::check_op` first. Never re-send an unconfirmed insert blind — read it back (`Session::insert_landed`). [→ evidence](../../notes/gotchas.md#an-insert-with-a-missing-anchor-or-over-the-cpu-budget-aborts-the-device-server)

- **A batched scene-leveling save can revert the ONE scene it just leveled**, if that scene equals `restore_scene` (the preset's on-load scene, recalled right before the save) — `loadScene(N)` re-instantiates scene N's STORED overlay, discarding N's own unsaved edit. Detected (not silent) by the existing `persist_mismatches` check; HW-reproduced twice, deterministic for any preset whose stored on-load scene is the one a batch levels. [→ evidence](../../notes/gotchas.md#a-batched-scene-leveling-save-can-revert-the-one-scene-it-just-leveled-if-that-scene-is-also-restore_scene--a-pre-existing-platform-agnostic-bug-not-a-linux-audio-defect)

## Re-amp and measurement

- **One engage per capture, on a fresh connection — for ACCURACY now, not safety.** Re-engaging on a held connection is SAFE on fw 1.8.58 (tmp-audit Q38: held-connection engages, re-engages and toggles all applied, no wedge, no USB or Mac fault; on fw 1.8.45 a held re-engage wedged re-amp and rebooted the Mac), but only the FIRST capture after an engage is accurate — later captures on one engage read up to 0.29 LU low. `leveller::level_preset` still uses **three fresh connections** (load / measure / apply). For SCENE and footswitch leveling only the measurement prepass reconnects (one engage per scene); that path's apply — per-scene `outputLevel` + save, all pure SENDS with no engage — runs on ONE persistent session. Either way the run must still end with a guaranteed re-amp OFF on its own fresh connection. See `notes/protocol.md`.

- **DANGER** — **A silent/failed re-amp inject reads as the device's STATIONARY OUTPUT FLOOR**, and `measure_c` would accept it as a valid `C` without the production floor guards. In a rapid 20-engage `probe --stim-ab` sweep, **19 of 20** captures measured the post-DSP floor rather than the stimulus. [→ evidence](../../notes/gotchas.md#a-silentfailed-re-amp-inject-reads-as-the-devices-stationary-output-floor-and-measure_c-would-accept-it-as-a-valid-c-without-the-production-floor-guards-below)

- **An engage whose LAST-command→engage idle gap exceeds ~300 ms is DEAD** — the capture reads the stationary floor exactly as if the inject failed. The discriminator is the GAP, not whether any command was sent: a 2026-08-31 bisect on a zero-write `--measure-pair` base reading latched the floor 2/2 at a ~600 ms gap even WITH an intervening `presetLevel` write present, while a single write that kept the gap at ~300 ms passed 1/1 — falsifying the older "any command between the recall and the engage rescues it" wording. Keep every idle gap ≤300 ms; `leveller::capture_on_session` (and `arm_pair_measurement`'s zero-write base arm) heartbeat the naked shape to do exactly that (engage ~900 ms post-recall). [→ evidence](../../notes/gotchas.md#an-engage-after-a-naked-scene-recall-latches-silence--break-the-idle-with-heartbeats)

- **Re-amp engages reliably only ONCE per connection** on fw 1.8.45; on fw 1.8.58 a held-connection re-engage applies (tmp-audit Q38). Fresh-connect per engage anyway (above). The `ReAmpModeChanged` echo is flaky and is NOT proof of engagement — a finite captured loudness is. [→ evidence](../../notes/gotchas.md#re-amp-engages-reliably-only-once-per-connection)

- **Re-amp latches preset state at engage** — the captured tap reflects only the `presetLevel` set BEFORE engaging. Set level, then engage.

- **Latch nuances (fw 1.8.45).** `changeParameter` IS audible mid-engage — live knob nudges work, and the whole live-leveling family rests on this. But `loadScene` mid-engage is INAUDIBLE: the active scene latches at engage (all 9 scene rows of an 8-scene preset measured identical audio on one engage), so per-scene leveling requires one engage per scene. Separately, `load_preset` + engage in the SAME connection captures pure silence — load in its own connection, then fresh-connect to set and engage (the `measure_knob_at` shape). _Keep these as distinct facts: `presetLevel` is a load-time latch, `changeParameter` is a mid-session live write._

- **Re-amp toggle** is `SettingsMessage(3) → reampModeActive(30)`, NOT MixerMessage. ON = `1a05f201020801`, OFF = `1a03f20100`.

- **Leveling runs must end with a GUARANTEED re-amp OFF on a fresh connection.** A dropped OFF strands the unit input-muted. Recovery: `cargo run --bin probe -- --reamp-off`. Cancel never early-returns past the re-amp engage for this reason, and the post-cancel restore/deferred-save cleanup always runs to completion.

- **`load_preset` + `set_preset_level` in the SAME connection → the set is overridden** by the load's own level-apply. Load in its own connection; a no-load set on the already-current preset sticks and persists across USB reconnects.

- **The scene-context rule: a bare write/measure with no preceding `loadScene` lands in whatever scene the connection currently holds, never base by default.** A preset loads into its SAVED `lastLoadedScene`. [→ evidence](../../notes/gotchas.md#the-scene-context-rule-a-bare-writemeasure-with-no-preceding-loadscene-lands-in-whatever-scene-the-connection-currently-holds-never-base-by-default)

- **OPEN — do not trust scene-0 leveling until resolved.** On the 2-amp Guitar preset, USB `loadScene(0)` materializes a different amp state than the physical footswitch tap.

- **`outputLevel`=0 is DEEP DIGITAL SILENCE on the real TMP**, and the leveling measurement ERRORS ("no signal captured") on a silent capture — a finite LUFS is not recoverable from silence. [→ evidence](../../notes/gotchas.md#outputlevel0-is-deep-digital-silence-on-the-real-tmp-and-the-leveling-measurement-errors-no-signal-captured-on-a-silent-capture)

- **48 kHz stimulus required** — that is the **host Core Audio rate** the device must be set to, not "the device clock" (macOS). On Linux there is no host-rate-vs-device-clock distinction to misconfigure: `hw:` negotiates the sample rate directly with the hardware (`pick_config`'s `target_rate`), bypassing any system-default-samplerate layer entirely — the TMP's `hw:` interface natively offers 48 kHz among its rates, confirmed HW-measured. [→ evidence](../../notes/gotchas.md#48-khz-stimulus-required)

## Resources and connections

- **DANGER** — **`audio::LiveReamp` is ring-buffered (`LIVE_RING_SECS`).** Its capture buffer once grew unboundedly (multi-channel 48 kHz × minutes × dozens of rows) and **OOM'd the whole Mac**. Never reintroduce unbounded capture accumulation; reuse ONE stream pair per preset rather than rebuilding per scene, which also avoids coreaudiod churn.

- **HID open-lockout model (HW-isolated, fw 1.8.45) — gone on fw 1.8.58.** On 1.8.45 a closed session allowed a QUICK re-open (≤ ~800 ms), then LOCKED OUT exclusive opens (`0xe00002c5`) for tens of seconds, and every failed open appeared to reset the lockout. On 1.8.58 there is none (tmp-audit Q36: 1,060 in-process open/close cycles at 0 ms–3 s gaps and after a 42.6-min idle, zero `0xe00002c5`): the error means another handle holds the seize — Pro Control, or a second `Session` in this process — and clears when it closes. The lockout rests and `hid.rs`'s retry ladder remain in code for now. [→ evidence](../../notes/gotchas.md#hid-open-lockout-model-hw-isolated-fw-1845)

- **Exclusive HID seize blocks Pro Control.** `IOHIDDeviceOpen` fails while Pro Control is running; the app surfaces a "close Pro Control" error.

## UI values

- **DANGER** — **`Pick`/`BlockPick` DISPLAY `options[0]` when `value` isn't in `options`** (`options.find(...) ?? options[0]`) — the UI shows one thing while the submitted value is another. Derive defaults from the live option source, never hard-coded ids: a store without a Crunch target displayed "Rhythm" while the run got `"crunch"`, a silent −30 fallback.
