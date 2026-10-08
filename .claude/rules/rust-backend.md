---
paths:
  - "src-tauri/**/*.rs"
---

# Rust backend rules

Applies while editing `src-tauri/`. Device-destructive and re-amp rules are in `danger.md` (always loaded); wire-format detail is the `tmp-companion-protocol` skill.

## Formatting — nothing does this for you

`main` is fmt-clean and CI's build-test gate runs `cargo fmt --check`, but **no local hook formats Rust**. Run `cargo fmt` before pushing any Rust change — extracted or moved code inherits its old nesting indentation and fails the gate otherwise. Single-file `rustfmt <file>` ERRORS on `async fn` without `--edition 2021` (bare rustfmt defaults to edition 2015).

## Module docs are the authority

Every backend module carries a `//!` header, and for the bulk/offline feature modules (reachable via the `probe` bin and tests, not the 6-tab UI) **those headers are authoritative** — prefer them over any prose summary elsewhere.

## Wire protocol

- **Setters/commands and the heartbeat OMIT `batchStatus`** — only _requests_ include it. `SetReAmpMode`, `SetPresetLevel`, `LoadPreset`, `SaveCurrentPreset` sent with a `batchStatus` are **silently ignored** by the device.
- **`batchStatus`: 0/absent answers at once, 1/2 only queue, 3 drains the queue, ≥4 is dropped** (fw 1.8.58 static RE, HW-confirmed). The queue outlives a session, so a 1/2 left behind is answered on the NEXT session. After the handshake, a request that needs its reply uses `proto::BATCH_DRAIN` or none; `extra_burst` debug-asserts it. [→ model](../../notes/protocol.md#handshake-batchstatus-grouping)
- **Live setters + import framing are byte-exact from a real Pro Control capture** — a PC rename is `renameCurrentPreset(13)` + `saveCurrentPreset(14)`; send both to persist. [→ evidence](../../notes/gotchas.md#live-setters--import-framing-byte-exact-from-a-real-pro-control-capture)
- **LIVE per-node structural edits** go through `bulk_replace_live` and `copy_apply`, sharing the `nodeReplaced(40)`/`nodeRemoved(36)`/`nodeInserted(33)` confirm gate, never-save-on-`presetError`(53), and the first-edit-after-load retry-harden. [→ evidence](../../notes/gotchas.md#live-per-node-structural-edits-the-protocol-behind-the-block-edit-features)
- **Firmware version read is in-burst `currentFwRequest`, NO `batchStatus`, in the batch-2 group** — after `userir_field2`, BEFORE `current_preset_data_request(batch=3)`. Sending it after batch-3 makes the device drop the reply; standalone after the burst gives a ConnectionError.
- **Preset-list reassembly needs both stream rules.** `list_my_presets` tries `streams()` and `streams_final()` and keeps the longest decoded record set. [→ evidence](../../notes/gotchas.md#preset-list-reassembly-also-needs-both-stream-rules)
- **`connect_for_discovery` (field-78) was effectively DEAD on fw 1.8.45** — it killed field-3 delivery for the whole session while its window was silent; kept alive it delivers on fw 1.8.58. [→ evidence](../../notes/gotchas.md#connect_for_discovery-field-78-is-effectively-dead-on-fw-1845)
- **Boot-window `IOHIDDeviceSetReport failed: 0xe00002d6` is NOT an error** — the HID interface enumerates ~20 s before the USB stack accepts reports. [→ evidence](../../notes/gotchas.md#boot-window-iohiddevicesetreport-failed-0xe00002d6-kioreturntimeout-is-not-an-error--its-device-not-ready-yet)
- **Serde casing exception (Copy):** `copy_apply`'s `CopyJob`/`CopyOp`/`CopyRepl` use **camelCase** nested keys because each field carries an explicit `#[serde(rename = "…")]` that OVERRIDES the enum's `rename_all = "snake_case"`. Verify a wire shape against the per-field attrs, not the enum-level `rename_all`.

## Connection and state

- **Never wait silently for a reply.** fw 1.8.58 drops the HID client after 0.75 s with no inbound frame and discards the reply in flight; a lapsed client ignores writes. Use `pump_collect_alive` / `send_and_collect_alive` / `pump_until_stable` for reply waits; `pump_silent` only where the silence is the point (drains, `lapse`). `TrackedHid` reopens a lapsed client before any write. [→ evidence](../../notes/gotchas.md#the-device-drops-the-hid-client-after-075-s-with-no-inbound-frame--and-discards-the-reply-in-flight-fw-1858-static-re--hw-ab)

- **`connect_device` releases any old HID seize before enabling the monitor.** It does not acquire a UI-owned session. [→ evidence](../../notes/gotchas.md#connect_device-releases-any-old-hid-seize-before-enabling-the-monitor)
- **Connection-perf fast paths:** `list_presets` answers from the startup snapshot with no HID or device-op lock. [→ evidence](../../notes/protocol.md#connection-perf-fast-paths)
- **Scene policy is pure-lazy, no cache** — no eager startup sweep. [→ evidence](../../notes/gotchas.md#scene-policy-is-pure-lazy-no-cache)
- **Logging:** `Builder::new()` already ships the default `[Stdout, LogDir]` targets and `.target()` **APPENDS** — you must `.clear_targets()` before re-adding, or every line double-logs. Runtime code uses `log::*`; the `probe`/`gen_samples` CLIs keep `println!` because stdout _is_ their interface.
- **macOS ships; Linux also ships (alpha); Windows is a supported dev platform.** `core-foundation`/`core-foundation-sys` are `cfg(target_os = "macos")`; `libc` is pulled by BOTH macOS and Linux (`hid.rs`'s hidraw transport); `windows-sys` by Windows (`hid.rs`'s Win32 HID transport). `hid.rs` and `audio.rs` each carry one `imp` per platform and NOTHING above that seam is platform-gated — `hid.rs` has IOKit / hidraw / Win32 `imp`s, `audio.rs` has CoreAudio / ALSA `hw:` / WASAPI `imp`s (ALSA resolves the device via `/proc/asound`; I32↔f32 conversion at the stream-callback boundary covers the integer-only hosts — the TMP's `hw:` interface has no F32 format). The `sqlite3` CLI is NOT a dependency: backup DBs are read in-process via `rusqlite` (bundled). `watcher.rs` (hotplug) has macOS and Linux `imp`s only — a no-op elsewhere. `release.yml` builds a Linux `.deb`/`.rpm` alongside the macOS DMG (never blocking it) and no Windows bundle — see `notes/linux-packaging.md`.
