# TMP USB protocol — load-bearing invariants

Facts about driving the Fender Tone Master Pro over exclusive-seize USB HID that are
**not obvious from the code** and were learned on real hardware or from the fw 1.8.58
firmware. Break one and the device goes silent, strands itself input-muted, mis-levels a
preset, or loses a reply. Golden-tested where possible (`proto.rs` vectors); the rest are
HW-validated. The danger rules are in `.claude/rules/danger.md`, their evidence in
`notes/gotchas.md`.

Cross-references: `src-tauri/src/session/` (`handshake.rs` + the command modules; `link.rs`/`timing.rs` hold the transport and every wait), `hid.rs` (the seize +
open-retry), `leveller/` / `audio.rs` (re-amp measurement), `monitor.rs` (the live
session), and `notes/write-safety.md` (the write/read-back story).

## Handshake `batchStatus` grouping

Requests carry a `batchStatus` field (FenderMessageTMS field 10). On fw 1.8.58 (static RE
of the server's message processor, HW-confirmed):

- `0` or absent → answered at once;
- `1` / `2` → only **queued**;
- `3` → queued, then the **whole queue is answered in order**;
- `4` and above → **dropped**, no reply.

The queue is device state: it survives the HID inactivity drop and a fresh
`connectionRequest`, and a later batch 3 sends the stale replies to whoever is on HID
then. Only a device backup or restore clears it (static RE, fw 1.8.58). A backup drops
anything else sent while it runs, and after a restore ends the unit answers only
connection and restore messages until it restarts.

The handshake mirrors Pro Control: `preset_list_request(1)` = 1; favorite /
`preset_list(4)` / `preset_list(3)` / `product_profile` / `current_preset_info` /
`settings66` / `userir` = 2; `current_preset_data_request` = 3, which drains the lot and
whose own reply (`currentPresetDataChanged`, field 3) comes last. **Every request after
the handshake uses `proto::BATCH_DRAIN` (3) or none**, so nothing is left queued for the
next session.

**Setters and the heartbeat OMIT `batchStatus`** — only _requests_ carry it. A
`SetReAmpMode` / `SetPresetLevel` / `LoadPreset` / `SaveCurrentPreset` sent _with_ a
`batchStatus` is silently ignored.

**Firmware version** rides the batch-2 group: `currentFwRequest`, no `batchStatus`, sent
after `userir_field2` and BEFORE `current_preset_data_request(batch=3)`. It is answered at
once, but sent after the batch-3 request its reply queues behind the ~1.2 s handshake
drain and a short window misses it (static RE, fw 1.8.58; drain timing HW).

**Sessions and pushes.** The unit closes a client after 750 ms with no OUT report; a write
sent after that reaches a closed client, draws `connectionError` and is dropped (HW, fw
1.8.58). Pushes are broadcast to every open client, so any heartbeat cadence well under
750 ms keeps receiving them; ~250 ms is a safety margin, not a subscription (static RE +
HW, fw 1.8.58). Details: [the 0.75 s client-drop entry](gotchas.md#the-device-drops-the-hid-client-after-075-s-with-no-inbound-frame--and-discards-the-reply-in-flight-fw-1858-static-re--hw-ab).

## Settings and Mixer reads

- Mixer read requests (TMS 5) are answered to the requesting host on a live, heartbeated
  session, and `reAmpModeRequest` (4) returns the real re-amp state (static RE + HW, fw
  1.8.58). A silent mixer read means the session had lapsed.
- `footswitchSettingsRequest` (Settings 47) is answered with `footswitchSettingsResponse`
  (48) to the requesting host. Its field 12 is `sceneChangeBehavior`: 0 = MAINTAIN CHANGES
  (left out on the wire), 1 = DISCARD CHANGES (static RE + HW, fw 1.8.58). The unit's own
  UI sets it with Settings `sceneChangeBehavior` (83), whose echo goes to every host (static
  RE, fw 1.8.58).

## Slot addressing: device userSlot = list index + 1

`list_my_presets` is **0-based**; the slot-addressed setters and the Song read's
`userPresetSlot` are **1-based**, and `session/preset.rs` owns the `+1`. The rule, and the
same-address-space guard every destructive op needs, are in `danger.md`; the evidence is
[the slot-addressing entry](gotchas.md#slot-addressing-device-userslot--list-index--1).

## Re-amp measurement (loudness leveling)

Re-amp mode replays a synthetic stimulus through a preset's DSP chain and captures the
processed USB-Out — no guitar plugged in. Toggle = `SettingsMessage(3) → reampModeActive`
(ON `1a05f201020801`, OFF `1a03f20100`), **not** a MixerMessage. The connection shapes
(three fresh connections per base level, one engage per capture, the guaranteed OFF), the
latch rules and their fw 1.8.58 status are `danger.md`'s "Re-amp and measurement"; the
leveling model is `notes/leveling.md`.

- **The active scene and the Scene-Edit state survive a reconnect** (static RE, fw 1.8.58).
  Any capture/measure path that addresses a scene still re-asserts `loadScene` on the SAME
  connection that engages re-amp (the Doctor capture, `capture_full_at`; the leveling
  `set_knob` re-asserts scene + scene-edit per connection). That is safe, except that a
  repeated enable on an overlay that is already Full reseeds it from base.
- **Working-copy edits survive a reconnect** (HW: a forced `bypass` measured the same on a
  fresh zero-write connection). Only a real `loadPreset` (a same-slot one only over a DIRTY
  working copy) or any `loadScene` under DISCARD CHANGES discards them; a save persists them,
  unsaved scene overlays included. A `loadScene` re-asserts the params that scene overlays,
  so a capture that recalls a scene must re-send its forced isolation every time.

## HID seize, and why there is no open lockout

The TMP is single-connection exclusive-HID (`kIOHIDOptionsTypeSeizeDevice` on macOS, share
mode 0 on Windows, `flock` on Linux); there is exactly ONE seize owner at any instant. On
macOS `IOHIDDeviceOpen` fails with `kIOReturnExclusiveAccess` (`0xe00002c5`) while Pro
Control holds the unit — surfaced as the `busy` connect error.

- **There is no device-side open lockout** (HW, fw 1.8.58). `0xe00002c5` means another
  handle holds the seize — Pro Control, or an overlapping `Session` in this process, which
  `Hid::open` names in its error. Diagnosis order and the retry constants:
  [the `0xe00002c5` entry](gotchas.md#0xe00002c5-means-another-handle-holds-the-seize--the-open-lockout-model-was-a-host-side-misdiagnosis).
- The same `0xe00002c5` also fires on **concurrent** device commands. Every device
  read/write is serialized process-wide by `DEVICE_OP_LOCK` (`device_gate.rs`, acquired
  inside each command's `spawn_blocking`).
- Boot-window `IOHIDDeviceSetReport failed: 0xe00002d6` is **not an error**: classify by
  error KIND (`not_ready`), never a retry count — see
  [the boot-window entry](gotchas.md#boot-window-iohiddevicesetreport-failed-0xe00002d6-kioreturntimeout-is-not-an-error--its-device-not-ready-yet).

## The monitor's pause/ack seize-sharing

While live-sync is enabled the monitor holds the seize; every command opens its own fresh
`Session`. They coexist via a pause-then-ack handshake gated by the same `DEVICE_OP_LOCK`:
a command sets `MONITOR_PAUSE_REQ` and waits (bounded) for `MONITOR_PAUSED_ACK`; the
monitor drops its `Session`, sets the ack, and parks until the request clears; the guard's
Drop clears the request and the monitor reconnects. The monitor never acquires the lock, so
the command's bounded _sleep_ on the ack is not a lock cycle → no deadlock.

## Complete-preset reads over USB

- `currentPresetDataChanged` (field 3, LZ4) is the whole working copy as one LZ4 block; a
  lapse mid-stream cuts it.
- `presetDataRequest` (field 8) → `presetDataChanged` (field 9, plaintext) returns the whole
  stored row when the session is kept alive (HW + static RE, fw 1.8.58: no cap below the
  65,535-byte message limit); a lapse cuts it, `scenes` first. It carries **no**
  `batchStatus`. On a quiet session `TrackedHid` re-arms the lapsed client with a
  `connectionRequest` first; on an ALREADY-LIVE (dense-heartbeat) session that re-arm only
  draws a `connectionError` — use `read_slot_preset_json_live` there.
- `exportPresetRequest` (field 115) is answered with `exportPresetResponse` (116): complete
  but encrypted.

Write paths still build from the OFFLINE `.preset` file or the backup (see
`notes/write-safety.md`); trust a USB read only when it ends in its final `0x35` frame. A
read sent during a flood queues behind it; keep the session alive until its final frame.

## Connection-perf fast paths

The live fast paths (`.claude/rules/rust-backend.md` carries the summary bullet):

- **Startup snapshot** — `connect_device` does not open HID itself; it enables the app-level monitor and waits for `StartupSnapshot { firmware, presets, graph }`, all from the monitor's single `connect_with_firmware()` handshake. `list_presets` answers from that snapshot with no HID or device-op lock, so list + chain can paint together. The snapshot's `presets` list is STRICT (`list_my_presets_strict`, see `.claude/rules/rust-backend.md`, preset-list reassembly).
  - **Idempotent vs an already-running monitor:** when `MONITOR_ENABLED` is already set (webview reload re-calls connect), it serves the cached snapshot instead of `reset_startup_state()`-ing it — the live pump never re-runs its one-shot handshake, so an unconditional reset would wait 8 s for a snapshot that can never be re-produced ("monitor startup timed out waiting for TMP handshake"). The cached `graph` is kept CURRENT by `monitor::refresh_snapshot_graph` on every field-3 push, so a reloaded webview paints the device's current preset, not the connect-time one.
  - **`graph=none` snapshots self-heal:** a flooded handshake can miss BOTH the field-3 push and the PresetLoaded body the field-8 fallback needs (`loaded_slot()` → None, silent), and an IDLE device never pushes field-3 on its own — so the hero would stay "No active preset" forever. The monitor does bounded re-snapshot retries (`GRAPH_RETRY_MAX`=2, 3 s backoff: drop session → re-handshake → fresh field-3 chance), keeps serving the list meanwhile, and refills the retry budget on every graph-ok snapshot + every pause/resume op cycle. Its startup graph fallback reads with `read_slot_preset_json_live` (no re-arm on the live session).
- **Skip-when-N/A** — `with_released_seize` skips its UI reconnect (and the `hid::wait_released` wait, ≤1 s) while the monitor owns the device; `lock_device_op` skips the pause-ack wait when no monitor can answer (live-sync off, or no monitor thread). The hid.rs bounded open-retry absorbs the residual seize-recycle lag.
- **`Hid::transact_eager`** (used ONLY by the fire-and-forget `load_preset`/`load_scene`, which discard their reports): pumps in 20 ms slices and exits once framing is complete (0x35-is-final fold) + 60 ms quiet, hard-capped at a 300 ms window — never slower, can't lose data a caller reads.
- **Songs reads are the slow path:** `read_song_list`/`read_setlist_list` fail closed and retry up to 10×, EACH retry a full fresh handshake (~660 ms).
