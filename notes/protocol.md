# TMP USB protocol — load-bearing invariants

Facts about driving the Fender Tone Master Pro over exclusive-seize USB HID that are
**not obvious from the code** and were learned on real hardware. Break one and the device
goes silent, strands itself input-muted, mis-levels a preset, or (worst case) wedges the
USB stack. Golden-tested where possible (`proto.rs` vectors); the rest are HW-validated.

Cross-references: `src-tauri/src/session.rs` (handshake + commands), `hid.rs` (the seize +
open-retry), `leveller.rs` / `audio.rs` (re-amp measurement), `monitor.rs` (the live
session), and `notes/write-safety.md` (the write/read-back story).

## Handshake `batchStatus` grouping

Requests carry a `batchStatus` field; the device answers a burst **only** if the host
mirrors Pro Control's exact grouping — it does **not** increment per request:

- `preset_list_request(filter=1)` → `batchStatus = 1`
- favorite / `preset_list(4)` / `preset_list(3)` / `product_profile` / `current_preset_info`
  / `settings66` / `userir` → all `batchStatus = 2`
- `current_preset_data_request` → `3`
- `current_preset_data_json_request` → `4`

Increment the batch on every request and the device goes silent after the first couple of
replies (observed: it answered the two preset lists, then nothing). So `list_my_presets`
worked but product profile / current-preset data / preset JSON never arrived.

**Setters and the heartbeat OMIT `batchStatus`** — only _requests_ carry it. A
`SetReAmpMode` / `SetPresetLevel` / `LoadPreset` / `SaveCurrentPreset` sent _with_ a
`batchStatus` is silently ignored.

**Firmware version** rides the batch-2 group: `currentFwRequest`, no `batchStatus`, sent
after `userir_field2` and BEFORE `current_preset_data_request(batch=3)`. Sent after batch-3
(or standalone after the burst) the reply is dropped.

## Slot addressing: device userSlot = list index + 1

`list_my_presets` is **0-based**. The slot-addressed setters — `loadPreset.presetSlot`,
`saveCurrentPreset.userSlot`, `clearUserPreset`, `moveUserPreset` — and the Song read's
`userPresetSlot` are all **1-based**. `session.rs` owns the translation
(`load_preset`/`save_current_preset`/`clear_user_preset`/`move_user_preset` take a 0-based
list index and send `+1`); every caller passes list indices. A guard on a destructive op
(clear / move / save-over) **must live in the same address space as the mutation** — an
earlier off-by-one deleted a real preset because the guard checked list-index space while
`clear` acted in 1-based device-slot space.

The empty-slot marker is `--` (also "Empty").

## Re-amp measurement (loudness leveling)

Re-amp mode replays a synthetic stimulus through a preset's DSP chain and captures the
processed USB-Out — no guitar plugged in. Toggle = `SettingsMessage(3) → reampModeActive`
(ON `1a05f201020801`, OFF `1a03f20100`), **not** a MixerMessage.

Load-bearing latch rules (fw 1.8.45):

- **Re-amp latches state at engage** — the captured tap reflects only the `presetLevel`
  set _before_ engaging. Set level → then engage.
- **`load_preset` + engage in the SAME connection captures silence.** Load in its own
  connection, drop, settle, then fresh-connect to set + engage (the `measure_knob_at`
  shape).
- **Re-amp engages reliably only ONCE per connection.** Fresh-connect per engage. The
  `ReAmpModeChanged` echo is flaky and is NOT proof of engagement — a finite captured
  loudness is.
- **Never re-engage on a held connection** (disengage → settle → re-engage): HW-observed to
  wedge the device's re-amp AND trigger a USB crash that rebooted the Mac. Only the
  measurement PREPASS reconnects (one engage/scene); the leveling APPLY (set `presetLevel` +
  per-scene `outputLevel` + save — all pure sends) runs on ONE persistent session.
- `changeParameter` IS audible mid-engage (live knob nudges work), but `loadScene`
  mid-engage is INAUDIBLE (the active scene latches at engage). Per-scene leveling therefore
  requires one engage per scene.
- **The active scene does NOT survive a reconnect** (the preset does). Any capture/measure
  path that addresses a scene must re-assert `loadScene` on the SAME connection that engages
  re-amp — loading it in a throwaway load connection (then reconnecting to capture) measures
  whatever scene the unit was already on. Bit the Doctor capture (`capture_full_at`); the
  leveling `set_knob` re-asserts scene + scene-edit per connection for the same reason.
- **`outputLevel = 0` is deep digital silence**; `loudest_loudness` errors ("no signal
  captured") on a silent capture — treat that error as a sentinel deep floor, never
  propagate it (else it aborts the scene).
- Leveling is **one-shot open-loop**: `captured_LUFS = 20·log10(level) + C`. Measure once,
  solve `C`, set the exact level. `presetLevel` (linear amplitude) is the global multiplier
  over all scenes → level the base scene FIRST, then each scene's amp `outputLevel`.
- **The 6 s stimulus + 0.8 s tail capture window is load-bearing** — TMP presets are not
  stationary under gated-integrated LUFS (reverb/delay build-up + decay tail), so
  early-exit / tail-drop / preroll-skip each shift the measured loudness preset-dependently
  (≤0.3 LU). Validate any measurement change against the full-capture oracle
  (`probe --measure-adaptive`), never a level→verify round-trip (self-consistent, hides the
  offset). 48 kHz stimulus required to match the device clock.

## HID seize + the open-lockout model

The TMP is single-connection exclusive-HID (`kIOHIDOptionsTypeSeizeDevice`); there is
exactly ONE seize owner at any instant. `IOHIDDeviceOpen` fails with
`kIOReturnExclusiveAccess` (`0xe00002c5`) if Pro Control is running — surfaced as a "close
Pro Control" error.

- After a session closes the device accepts a QUICK re-open (≤~800 ms) but then **locks out
  exclusive opens for tens of seconds**, and **every failed attempt appears to RESET the
  lockout** — hammering retries never recovers; only a long quiet does. `hid.rs` retries at
  two levels: a same-ref fast lane (6×80 ms) then full re-enumeration retries (3×8 s quiet
  backoff; a stale device ref fails forever).
- The same `0xe00002c5` also fires on **concurrent** device commands. Every device
  read/write is serialized process-wide by `DEVICE_OP_LOCK` (acquired inside each command's
  `spawn_blocking`). Front-end serialization alone is insufficient — the release→work→
  reconnect churn lets two _operations_ overlap across a tab switch.
- Boot-window `IOHIDDeviceSetReport failed: 0xe00002d6` (kIOReturnTimeout) is **not an
  error** — the HID interface enumerates ~20 s before the USB stack accepts reports, so the
  first cold-boot handshake _send_ times out. Classify by error string, not a retry count
  (`connectError.ts`).

## The monitor's pause/ack seize-sharing

While live-sync is enabled the monitor holds the seize; every command opens its own fresh
`Session`. They coexist via a pause-then-ack handshake gated by the same `DEVICE_OP_LOCK`:
a command sets `MONITOR_PAUSE_REQ` and waits (bounded) for `MONITOR_PAUSED_ACK`; the
monitor drops its `Session`, sets the ack, and parks until the request clears; the guard's
Drop clears the request and the monitor reconnects. The monitor never acquires the lock, so
the command's bounded _sleep_ on the ack is not a lock cycle → no deadlock.

## No reliable complete-preset read over USB

There is **no** USB path that returns a byte-complete preset:

- `currentPresetDataChanged` (field 3, LZ4) is a partial that truncates inside the scenes;
  session-health-dependent (~3.4 KB lean vs ~17 KB on a dense-heartbeat session).
- `presetDataRequest` (field 8) → `presetDataChanged` (field 9, plaintext) is a
  per-slot-deterministic partial (cut inside the final scene). Must carry **no**
  `batchStatus`; on a QUIET line re-arm the burst state with a leading `connection_request`,
  but on an ALREADY-LIVE (dense-heartbeat) session the re-arm draws a `connectionError` —
  use `read_slot_preset_json_live` there.
- `exportPresetRequest` (field 115) gets no companion-replayable response.

**Verdict: the canonical full-preset source is the OFFLINE `.preset` file** (see
`notes/write-safety.md`). The device answers exactly ONE data request per burst state; a
read fired mid-flood is dropped device-side — fire reads on a quiet line.

## Connection-perf fast paths

The five connection-perf lanes and their HW status (CLAUDE.md carries the summary bullet):

- **Connection-perf fast paths.** Status: path (1) is HW-green (incl. idempotent reconnect + strict list + graph-retry); the metadata lane (3) was exercised live with the caveats below; the live load lane (2) and `transact_eager` (5) remain HW-unmeasured.
  - **(1) startup snapshot** — `connect_device` does not open HID itself; it enables the app-level monitor and waits for `StartupSnapshot { firmware, presets, graph }`, all from the monitor's single `connect_with_firmware()` handshake. `list_presets` answers from that snapshot with no HID or device-op lock, so list + chain can paint together. The snapshot's `presets` list is STRICT (`list_my_presets_strict`, see CLAUDE.md's preset-list-reassembly gotcha).
    - **Idempotent vs an already-running monitor:** when `MONITOR_ENABLED` is already set (webview reload re-calls connect), it serves the cached snapshot instead of `reset_startup_state()`-ing it — the live pump never re-runs its one-shot handshake, so an unconditional reset would wait 8 s for a snapshot that can never be re-produced ("monitor startup timed out waiting for TMP handshake"). The cached `graph` is kept CURRENT by `monitor::refresh_snapshot_graph` on every field-3 push, so a reloaded webview paints the device's current preset, not the connect-time one.
    - **`graph=none` snapshots self-heal:** a flooded handshake can miss BOTH the field-3 push and the PresetLoaded body the field-8 fallback needs (`loaded_slot()` → None, silent), and an IDLE device never pushes field-3 on its own — so the hero would stay "No active preset" forever. The monitor does bounded re-snapshot retries (`GRAPH_RETRY_MAX`=2, 3 s backoff: drop session → re-handshake → fresh field-3 chance), keeps serving the list meanwhile, and refills the retry budget on every graph-ok snapshot + every pause/resume op cycle.
  - **(2) live command lane** — while the monitor is live, `load_preset_on_amp` / `load_scene_on_amp` (active-preset case only) execute on the MONITOR's persistent session (`monitor::try_live_op` → `send_and_collect`, the `probe --scenes-load` precedent) instead of the release→fresh-handshake→reconnect bookend (~2 s → ~0.2 s); every non-pumping monitor state drains the lane with `NotLive` and the command falls back to the classic path. ⚠️ **UI-ORPHANED** — app-driven preset recall was removed, so `load_preset_on_amp`/`load_scene_on_amp` + their `loadPresetOnAmp`/`loadSceneOnAmp` invoke wrappers have NO `src/` caller outside tests; the lane is kept as the API but is not wired to any UI gesture. The `Some(list_index)` scene case stays classic (load-overrides-scene in one connection is untested on a long-lived session).
  - **(3) metadata read lane** — `read_preset_scenes` first tries `monitor::try_metadata_read` (field-8 read on the monitor session), then falls back to the classic pause+fresh-session path on `NotLive`/no data. HW-exercised: reads WORK on the live session and the heartbeat survives. The per-read `connectionError` is the `connection_request` RE-ARM, not a busy line — a live session is already armed by its dense heartbeat, so re-arming it makes the device answer the next heartbeat with a `connectionError` (HW: 1/read, ~140 ms slower). FIXED — the live callers (`exec_metadata_read`, `startup_live` graph fallback) call `read_slot_preset_json_live`, which SKIPS the re-arm (and keeps the heartbeat alive through the harvest) → 0 `connectionError`/read (HW `probe --slotread-live <slot> [rounds]`, 10/10, same field-9 payload); the dedicated/quiet `scan_preset_scenes` sweep keeps the re-arm via `read_slot_preset_json`. (The earlier ~3.9 s heartbeat-STARVATION theory failed on HW: every slot answers in ~0.8 s and the read never runs the full 24-slice harvest, so the `connectionError` count is identical with vs without the keepalive heartbeat — proving it is the re-arm, not a gap.) ⚠️ `read_preset_scenes` is **UI-ORPHANED** (the LevelDialog uses the batch sweep; its only caller is the invoke contract test), making the whole metadata-lane machinery consumer-less — kept as the single-preset API.
  - **(4) skip-when-N/A** — `with_released_seize` skips the 400 ms settle + `lock_device_op` skips the 1 s pause-ack wait when they don't apply (monitor on / monitor disabled respectively); the hid.rs bounded open-retry absorbs the residual seize-recycle lag.
  - **(5) `Hid::transact_eager`** (used ONLY by the fire-and-forget `load_preset`/`load_scene`, which discard their reports): pumps in 20 ms slices and exits once framing is complete (0x35-is-final fold) + 60 ms quiet, hard-capped at a 300 ms window — never slower, can't lose data a caller reads.
  - **Known hotspot:** `read_song_list`/`read_setlist_list`'s fail-closed retries go up to 10×, and EACH retry is a full fresh handshake (~660 ms) — Songs-tab first paint is 5.4–9.4 s worst case.
