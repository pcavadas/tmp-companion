---
name: tmp-companion-device-ops
description: "How to drive the Tone Master Pro over USB from the Rust backend without wedging the device or making it go silent. Use this skill when adding or editing a #[tauri::command] that touches the device, or when working in hid.rs, session.rs, monitor.rs, leveller.rs, audio.rs, or lib.rs. Covers the load-bearing protocol facts as they are USED (the wire codec itself lives in proto.rs + notes/protocol.md): the process-global device-op lock, the exclusive-HID seize + open-lockout retry model, the batchStatus grouping rule, slot = index + 1, and the re-amp engage-once-per-connection rule. Read notes/protocol.md for the full protocol; this is the operating playbook."
---

# TMP Companion device ops

The TMP is a single-connection, exclusive-HID device with several non-obvious protocol rules; violating them makes it go silent, wedge, or (worst case) reboot the host. `notes/protocol.md` is the reference body; `CLAUDE.md`'s Gotchas are the authoritative running log. This skill is the operating playbook for the _as-used_ protocol — not how it was discovered.

## The rules

1. **Serialize every device op.** The device is single-connection exclusive-HID; two concurrent device-touching commands collide with `kIOReturnExclusiveAccess` (`0xe00002c5`). A process-global **`DEVICE_OP_LOCK`** is acquired inside every device command's `spawn_blocking` (`lib.rs`). Frontend serialization alone is insufficient — never `Promise.all` two device reads; `await` sequentially.

2. **HID seize + open-lockout.** The seize (`kIOHIDOptionsTypeSeizeDevice`) blocks Pro Control — surface a "close Pro Control" error, not a retry, on `0xe00002c5` at open. After a close, the device accepts a quick re-open (~≤800 ms) then **locks out** exclusive opens for tens of seconds, and every _failed_ open resets that lockout — so `hid.rs` retries at two levels (fast same-ref lane, then re-enumeration with a quiet backoff). Do not hammer retries.

3. **Setters omit `batchStatus`; requests reuse Pro Control's groups.** Only _requests_ carry `batchStatus`. A setter/command/heartbeat sent **with** a `batchStatus` is silently ignored (SetReAmpMode, SetPresetLevel, LoadPreset, SaveCurrentPreset). And requests must **reuse** Pro Control's exact batch-group values, not increment per request — increment it and the device answers the first couple then goes silent.

4. **Slot = list index + 1.** `list_my_presets` is 0-based; the slot-addressed setters (`load_preset` / `save_current_preset` / `clear_user_preset` / `move_user_preset`) send `+1`. `session.rs` owns this translation; callers pass 0-based list indices. (See `tmp-companion-write-safety` for the destructive-op guard rule.)

5. **Re-amp engages reliably ONCE per connection.** Fresh-connect per engage; a finite captured loudness — not the flaky `ReAmpModeChanged` echo — is the proof of engagement. **Never** disengage→re-engage on a held connection: it wedged the device and rebooted the Mac. Set `presetLevel` **before** engaging (re-amp latches state at engage). `load_preset` + engage in the _same_ connection captures silence — load in its own connection.

6. **Classify connect errors by string, not retry count.** Boot-window `IOHIDDeviceSetReport failed: 0xe00002d6` (kIOReturnTimeout) means "device not ready yet" — the friendly "power it on" gate, not the red banner. Only the exclusive-access `IOHIDDeviceOpen … 0xe00002c5` gets the red "close Pro Control" banner. `connectError.ts` classifies by matching the error text; if you reword a backend HID error, update the matcher.

## Where things live

`src-tauri/src/`: `hid.rs` (seize + retry), `session.rs` (handshake + commands + slot translation), `monitor.rs` (live session + fast paths), `leveller.rs` / `audio.rs` (re-amp), `lib.rs` (the commands + `DEVICE_OP_LOCK`), `proto.rs` (wire codec). Reference doc: `notes/protocol.md`.
