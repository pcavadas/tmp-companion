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
- **Requests must REUSE `batchStatus` in Pro Control's groups, not increment per request.** [→ evidence](../../notes/gotchas.md#requests-must-reuse-batchstatus-in-pro-controls-groups-not-increment-per-request)
- **Live setters + import framing are byte-exact from a real Pro Control capture** — a PC rename is `renameCurrentPreset(13)` + `saveCurrentPreset(14)`; send both to persist. [→ evidence](../../notes/gotchas.md#live-setters-import-framing-byte-exact-from-a-real-pro-control-capture)
- **LIVE per-node structural edits** go through `bulk_replace_live` and `copy_apply`, sharing the `nodeReplaced(40)`/`nodeRemoved(36)`/`nodeInserted(33)` confirm gate, never-save-on-`presetError`(53), and the first-edit-after-load retry-harden. [→ evidence](../../notes/gotchas.md#live-per-node-structural-edits-the-protocol-behind-the-block-edit-features)
- **Firmware version read is in-burst `currentFwRequest`, NO `batchStatus`, in the batch-2 group** — after `userir_field2`, BEFORE `current_preset_data_request(batch=3)`. Sending it after batch-3 makes the device drop the reply; standalone after the burst gives a ConnectionError.
- **Preset-list reassembly needs both stream rules.** `list_my_presets` tries `streams()` and `streams_final()` and keeps the longest decoded record set. [→ evidence](../../notes/gotchas.md#preset-list-reassembly-also-needs-both-stream-rules)
- **`connect_for_discovery` (field-78) is effectively DEAD on fw 1.8.45** — it kills field-3 delivery for the whole session. [→ evidence](../../notes/gotchas.md#connect_for_discovery-field-78-is-effectively-dead-on-fw-1845)
- **Boot-window `IOHIDDeviceSetReport failed: 0xe00002d6` is NOT an error** — the HID interface enumerates ~20 s before the USB stack accepts reports. [→ evidence](../../notes/gotchas.md#boot-window-iohiddevicesetreport-failed-0xe00002d6-kioreturntimeout-is-not-an-error-its-device-not-ready-yet)
- **Serde casing exception (Copy):** `copy_apply`'s `CopyJob`/`CopyOp`/`CopyRepl` use **camelCase** nested keys because each field carries an explicit `#[serde(rename = "…")]` that OVERRIDES the enum's `rename_all = "snake_case"`. Verify a wire shape against the per-field attrs, not the enum-level `rename_all`.

## Connection and state

- **`connect_device` releases any old HID seize before enabling the monitor.** It does not acquire a UI-owned session. [→ evidence](../../notes/gotchas.md#connect_device-releases-any-old-hid-seize-before-enabling-the-monitor)
- **Connection-perf fast paths:** `list_presets` answers from the startup snapshot with no HID or device-op lock. [→ evidence](../../notes/protocol.md#connection-perf-fast-paths)
- **Scene policy is pure-lazy, no cache** — no eager startup sweep. [→ evidence](../../notes/gotchas.md#scene-policy-is-pure-lazy-no-cache)
- **Logging:** `Builder::new()` already ships the default `[Stdout, LogDir]` targets and `.target()` **APPENDS** — you must `.clear_targets()` before re-adding, or every line double-logs. Runtime code uses `log::*`; the `probe`/`gen_samples` CLIs keep `println!` because stdout _is_ their interface.
- **macOS-only:** `core-foundation` / `core-foundation-sys` / `libc` are `cfg(target_os = "macos")`; the IOKit and cpal CoreAudio paths don't build elsewhere.
