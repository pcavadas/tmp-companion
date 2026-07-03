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
