# Device sessions and transport

Read this when you touch `hid.rs`, `src-tauri/src/session/`, `monitor.rs`, `device_gate.rs` or any wait that keeps a connection alive.
It covers how a host connection to the Tone Master Pro opens, stays alive, queues replies and shares the device.

Facts are for firmware 1.8.58 unless a line says otherwise.

## Transports by platform

The protocol bytes are the same on every platform; `proto.rs` and its golden vectors are platform-independent.

### macOS

The app opens the unit with `kIOHIDOptionsTypeSeizeDevice`, an exclusive seize ([Exclusive seize](#exclusive-seize)).
Inbound reports are 64 bytes and begin with the device's own `0x00`, so inbound frames parse as `[0x00, magic, _, body_len, …]` while outbound frames put the magic at index 0 (`proto::make_envelope`) [HW].

### Linux (hidraw)

Inbound hidraw reports also arrive as `n=64` beginning with a literal `0x00`, so `hid.rs`'s Linux `imp` pushes them raw [HW].
Do not add a report-id normalization layer; `hid.rs`'s `a_real_inbound_report_needs_no_report_id_fixup` test pins the measured shape.
`proto::reassemble_streams` already skips anything that fails `len >= 4 && data[0] == 0x00`, so the transport filters nothing.
hidraw is NOT exclusive: two `probe` processes both opened the node and both proceeded [HW].
The Linux `imp` therefore takes an advisory `flock(fd, LOCK_EX | LOCK_NB)` right after `open` and reports `EWOULDBLOCK` as "already in use by another TMP Companion process"; every device path goes through that one function, and the lock releases on close, abnormal exit included.
There is no open-retry loop on Linux: nothing holds a seize, Fender ships no Pro Control for Linux, and back-to-back `probe` sessions reconnect at once [HW].
`/dev/hidraw*` is root-only, so the first run fails `EACCES` until `packaging/udev/70-fender-tone-master-pro.rules` is installed.
That rule tags the node `uaccess`, which grants the logged-in user through a POSIX ACL; check it with `getfacl`, since the mode bits still read `crw-------`.
`udevadm trigger` re-applies it to a connected unit, so no replug is needed.
The unit enumerates two USB interfaces, `1ed8:0044` and `1ed8:0047`; only `0044` carries this protocol (`hid::PID`), and the rule tags both [HW].
The node is found by walking `/sys/class/hidraw/*/device/uevent` for `HID_ID=<bus>:<vid>:<pid>` (zero-padded uppercase hex), with no `udev` crate and no C `hidapi`.

### Windows

The HID descriptor declares 65-byte reports in and out, and Win32 carries the report ID as a leading byte both ways [HW].
So the 63-byte outbound frame is written as `0x00` + frame, zero-padded to 65, and the inbound 65 bytes are a `0x00` report ID followed by the device's own 64-byte report, whose first byte is also `0x00`.
The Windows `imp` strips exactly one leading byte, keyed on the descriptor length rather than the value; do not strip two.
`CreateFileW` with share mode 0 is the seize: against a running Pro Control the open fails `ERROR_SHARING_VIOLATION` (os error 32), surfaced with the "close Pro Control" hint, and nothing wedges [HW].

## Exclusive seize

The unit serves one HID host at a time, and the app holds an exclusive handle: the macOS seize, share mode 0 on Windows, `flock` on Linux.
On macOS `IOHIDDeviceOpen` fails `kIOReturnExclusiveAccess` (`0xe00002c5`) only while another handle holds the seize, and it clears once that handle closes [HW].
There is no device-side open lockout: 1,060 open → request → close cycles at gaps of 0 ms to 3 s, and an open after a 42.6-minute idle, all opened first try [HW+RE].
The two holders are Pro Control (the `busy` connect error, "Another app is using your Tone Master Pro") and an overlapping `Session` in this process, which `Hid::open`'s error names.
A `connect_device` that acquired before releasing would collide with its own stale handle, so it clears `AppState.session` first, then sets `MONITOR_ENABLED` and waits for the monitor's startup snapshot.
Two device commands run at once also collide, so every device command takes the process-global `DEVICE_OP_LOCK` (`device_gate.rs`) inside its `spawn_blocking`; front-end serialization alone is not enough, because `with_released_seize`'s release → work → reconnect lets two operations overlap.
`hid.rs` retries the same device reference 6 × 80 ms, then re-enumerates 3 × 400 ms (`ENUM_RETRY_DELAY_MS`), because a stale macOS IOHIDDevice reference can keep failing `0xe00002c5` while a fresh enumeration opens at once [HW].
Sessions reopen with no fixed gap; the UI's drop-then-reopen waits on `hid::wait_released`.

### Diagnosing `0xe00002c5`

1. Believe the error text first: Pro Control holding the unit is the common cause, and Pro Control driving the unit flawlessly is exactly what that looks like.
2. Then look for an overlapping `Session` in this process; `Hid::open`'s error names it.
3. A plain `probe` list that succeeds proves the unit free and healthy; no state needs a power cycle.

### Teardown

Unschedule the device from the run loop before `IOHIDDeviceClose`, and add no "drain until quiet" pump before close.
A `probe` that hangs before printing anything is a wedged connect (`hid.rs`'s `rx.recv()` has no timeout); kill it and retry once.

## Session lapse

The unit closes the HID client after 750 ms with no OUT report from the host (`HID_INACTIVITY_DROP_MS`) [HW+RE].
Every OUT report refreshes the timer, a `ConnectionHeartbeat` included, which queues no reply; reading IN does not count [RE].
An engage sent 740 ms after the last OUT report applied, and one sent at 760 ms was dropped [HW].
The silence is counted from the host's last report, not its last command, so a host that reads for 400 ms after each send has already used most of the window.
On a closed client every message except a `connectionRequest` draws `connectionError` (0) and is dropped, and only a `connectionRequest` reopens it; heartbeats sent after a lapse do not [HW+RE].
A `connectionRequest` on an open client draws `connectionError` (1) and is not dispatched [RE].
The lapse also resets the HID reply queue ([Reply queues](#reply-queues)), so it cuts any reply still streaming.
Pushes are broadcast to every open client, so a heartbeat cadence well under 750 ms keeps receiving them; a lapsed host misses every broadcast until it reconnects [HW+RE].

### How the code keeps a session alive

`TrackedHid` (`session/link.rs`) tracks host silence.
Reply waits pump with heartbeats (`pump_collect_alive`, `pump_until_stable`).
Before a write after `CLEARLY_LIVE_MS` (600 ms) of silence, `TrackedHid` re-opens with a `connectionRequest`; only the slot-read experiments turn that off (`set_auto_reopen(false)`).
Deliberate silence is explicit: `pump_silent`, `drain_until_quiet`, `Session::lapse`.
Keep every idle gap before a write or an engage at 300 ms or less; that proxy leaves margin under the 750 ms limit.
List harvests heartbeat until the list is proven, then pause past the timeout so callers get the lapsed session they expect; `list_my_presets_strict` heartbeats with no lapse, because the monitor keeps that session live.

## Request batching

Requests carry `batchStatus` (FenderMessageTMS field 10), and the unit treats it as follows [HW+RE]:

- `0` or absent: the message is dispatched at once.
- `1` or `2`: the message is queued until a later batch `3` arrives.
- `3`: the message is queued, then the whole queue is dispatched in order, each item answering its stored originator.
- `4` and above: the message is dropped with no reply.

The only reply any message can draw from a closed session is `connectionError` ([Session lapse](#session-lapse)).
Setters and the heartbeat omit `batchStatus`; requests carry it.
Every request after the handshake uses `proto::BATCH_DRAIN` (3) or none, so nothing is left queued for the next session.
`batchStatus` governs only how a request is admitted; a reply carries none.

## Handshake

The first-connect handshake mirrors Pro Control: `connectionRequest`, the preset lists, the product profile, and the current-preset, settings and user-IR requests.
Do not trim it without testing on the unit.
The groups are: `preset_list_request(1)` with batch 1; favorites, `preset_list(4)`, `preset_list(3)`, `product_profile`, `current_preset_info`, `settings66` and `userir` with batch 2; `current_preset_data_request` with batch 3, which drains the lot.
Its own reply, `currentPresetDataChanged` (field 3), comes last.
Nothing is pushed on connect: the unit answers a `connectionRequest` with the connection response only [RE].
The drain is about 567 frames, about 1.2 s at the unit's frame rate, and grows with the library names and the current preset's body [HW].
`currentFwRequest` rides the batch-2 group with no `batchStatus`, sent after `userir_field2` and before the batch-3 request: it is answered at once, but sent after the batch-3 request its reply queues behind the drain and a short window misses it [HW+RE].
Song reads ride the handshake burst with batch 3 (`song_io.rs`).

## Reply queues

The unit holds two queues, and they live and die differently.

### Batch request queue

Batch-1 and batch-2 requests wait in the MessageProcessor's own queue [RE].
It survives a lapse and a fresh `connectionRequest`, and a later batch 3 sends the stale replies to whichever host is on HID then [RE].
Only a device backup or restore clears it [RE].
A backup drops every other message sent while it runs, and once any restore ends the unit answers only connection and restore messages until it restarts [RE].

### HID reply queue

Every reply and push goes through one FIFO that frames one whole message at a time, so frames of two messages never interleave and a host-side cut is a drop, not a reassembly race [RE].
The unit sends one 64-byte report per ~2.14 ms (about 466 reports/s, 28 KB/s) whether or not a host has the HID open, so reply time is service time plus the frames queued ahead of it times 2.14 ms [HW+RE].
A lapse resets this queue: the message being framed and every queued reply are dropped [HW+RE].
Treat a stream without its final `0x35` frame as incomplete.

### Inheritance across sessions

A session opened within 750 ms of the last one's final OUT report inherits the rest of this queue, ahead of its own replies [HW].
Its `connectionRequest` draws `connectionError` (1) instead of a `connectionResponse`, because the old session is still open ([Session lapse](#session-lapse)) [HW+RE].
Reports sent while nothing is open are lost: a session closed right after its lean handshake and reopened G ms later lost about 1 + 0.48·G reports as one contiguous run, and the old flood's tail, its field-3 copy included, arrived before the new session's connection reply [HW].
A gap of 750 ms or more gives the next session a clean flood [HW].
A reader that keeps the longest carrier then adopts the stale inherited copy.
So `leveller::restore_saved_preset` and `load_preset_isolated` (behind `make_current`) end with `Session::lapse`, and `Session::latest_json_payload` takes the latest preset-JSON carrier, not the longest.
The offline fake models the queue as the `queued-handshake-flood` injection (`rig_tests.rs`).

## Boot window

On a cold boot the HID interface enumerates about 20 s before the unit's USB stack accepts reports, so the open, the seize and the hotplug watcher all succeed and the first handshake send times out: `IOHIDDeviceSetReport failed: 0xe00002d6` (`kIOReturnTimeout`) [HW].
That is "not ready yet", not an error.
`connect_device` returns a typed `error::AppError` (`{ kind, message }`), and `src/lib/connectError.ts` classifies `not_ready` and `not_connected` as the plain "Plug in your Tone Master Pro" page (`views/DeviceState.tsx`).
`busy` (a held exclusive open, or the macOS `kIOReturnNotPermitted` / Windows `ERROR_ACCESS_DENIED` refusal, `DeviceError::OpenDenied`) gets "Another app is using your Tone Master Pro", and every other kind gets "Couldn't connect to your Tone Master Pro".
Classify by error kind; never key a retry or debounce on a count, because the window spans several retries.
The UI never shows the backend message (it goes to the log), so rewording a backend HID error is safe.

## Monitor and automatic connection

There are no Connect, Disconnect or Scan buttons.
On mount the frontend calls `connect()` and retries every `RETRY_MS` (3 s, `App.tsx`) through one single-flight `attempt()` until the unit is found; the hotplug attach is the only other connect path.
`watcher.rs` emits `tmp://device-{attached,detached}`: on macOS through a non-seizing IOHIDManager, on Linux by polling `hid::device_present()` once a second, on Windows through non-seizing Configuration Manager HID notifications plus one-second SetupAPI presence polls.
On detach `App.tsx` clears the firmware and snapshot state and the 3 s retry stays armed, so a replug reconnects on its own.
The toolbar's `DeviceStatus` reads "Not connected", "Connecting…", "In use by another app", "Connected · firmware <fw>" or "Untested firmware <fw>".

### Startup snapshot

`connect_device` opens no HID itself: it enables the monitor and waits for `StartupSnapshot { firmware, presets, graph }`, all from the monitor's one `connect_with_firmware()` handshake.
`list_presets` answers from that snapshot with no HID and no device lock.
The snapshot's preset list is strict (`list_my_presets_strict`, [Reading presets](presets.md#reading-presets)).
With `MONITOR_ENABLED` already set (a webview reload), `connect_device` serves the cached snapshot instead of `reset_startup_state()`, because the live pump never re-runs its one-shot handshake.
`monitor::refresh_snapshot_graph` keeps the cached `graph` current on every field-3 push.
The app reads scenes lazily: the active preset from field 3, other rows from the startup backup scan (`BackupPresetRow.scenes`); nothing loads a preset at startup or during setup.
A flooded handshake can miss both the field-3 push and the `PresetLoaded` body, and an idle unit never pushes field 3 on its own, so the monitor retries the snapshot (`GRAPH_RETRY_MAX` = 2, 3 s apart) and refills that budget on every snapshot with a graph and every pause/resume cycle.

### Sharing the seize with commands

While the monitor runs it holds the seize, and every command opens its own `Session`.
A command sets `MONITOR_PAUSE_REQ` and waits, bounded, for `MONITOR_PAUSED_ACK`; the monitor drops its `Session`, acks and parks until the request clears; the guard's Drop clears it and the monitor reconnects.
The monitor never takes `DEVICE_OP_LOCK`, so the command's bounded wait on the ack cannot deadlock.
`with_released_seize` skips its UI reconnect and the `hid::wait_released` wait while the monitor owns the unit, and `lock_device_op` skips the pause wait when no monitor can answer.
Calibration from the Settings window works because every device command releases and re-acquires the seize.

### Fast and slow paths

`Hid::transact_eager` serves only the fire-and-forget `load_preset` and `load_scene`: it pumps in 20 ms slices and exits once framing is complete plus 60 ms of quiet, capped at 300 ms.
`read_song_list` and `read_setlist_list` fail closed and retry up to 10 times, each retry a fresh handshake.

## Pro Control framing

Outbound multi-packet messages use `0x33` (start), `0x34` (continue) and `0x35` (final) frames, each `MAGIC 00 LEN` with at most 60 payload bytes, symmetric with inbound [HW].
The single-packet setters, byte-exact from a Pro Control capture [HW]:

- `renameCurrentPreset` (13) `{displayName = f1}`; Pro Control renames with `renameCurrentPreset` + `saveCurrentPreset` (14) `{slot = f1}`, and both are needed to persist.
- `moveUserPreset` (16) `{old = f1, new = f2}` persists alone.
- `clearUserPreset` (15) `{slot = f1}`.
- `importPresetRequest` (117) `{presetJson = f1}`, where `presetJson` is LZ4-block(the raw `.preset` file bytes), that is LZ4 of the XOR-encoded compact JSON; Pro Control follows it with `loadPreset`.

Slot numbers on the wire are 1-based ([Slot addressing](presets.md#slot-addressing)).
