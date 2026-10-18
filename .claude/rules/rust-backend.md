---
paths:
  - "src-tauri/**/*.rs"
---

# Rust backend rules

Applies while editing `src-tauri/`.
Device-destructive and re-amp rules are in `danger.md` (always loaded); wire-format detail is the `tmp-companion-protocol` skill.

## Formatting — nothing does this for you

`main` is fmt-clean and CI's build-test gate runs `cargo fmt --check`, but **no local hook formats Rust**.
Run `cargo fmt` before pushing any Rust change — extracted or moved code inherits its old nesting indentation and fails the gate otherwise.
Single-file `rustfmt <file>` ERRORS on `async fn` without `--edition 2021` (bare rustfmt defaults to edition 2015).

## Module docs are the authority

Every backend module carries a `//!` header, and for the bulk/offline feature modules (reachable via the `probe` bin and tests, not the 6-tab UI) **those headers are authoritative** — prefer them over any prose summary elsewhere.

## Wire protocol

- **`batchStatus` (field 10): 0, 3 or absent apply at once (3 also drains the queue), 1 and 2 queue until a batch 3, ≥4 is dropped with no reply.**
  The queue outlives a session, so a 1 or 2 left behind is answered on the next session.
  Setters, commands and the heartbeat are sent without one; after the handshake, a request that needs its reply uses `proto::BATCH_DRAIN` or none (`extra_burst` debug-asserts it).
  [→](../../notes/device/session.md#request-batching)
- **A Pro Control rename is `renameCurrentPreset` (13) + `saveCurrentPreset` (14)**: send both to persist; the byte-exact setter and import framing is the `tmp-companion-protocol` skill.
- **Live per-node structural edits** go through `copy_apply` and the probe's `replace_many_held`, sharing the `nodeReplaced` (40) / `nodeRemoved` (36) / `nodeInserted` (33) confirm gate and never saving after a `presetError` (53).
  `copy_apply` reads an unconfirmed first edit back from the working copy and re-sends only one that visibly did not land; `replace_many_held` never re-sends.
  No handler drops a correctly framed structural edit, so a missing confirm is one still queued, a lapse, a backup freeze or an insert abort, and a blind re-send duplicates it [RE, inferred].
  [→](../../notes/device/presets.md#structural-edits)
- **The firmware version read is an in-burst `currentFwRequest` with no `batchStatus`**, sent after `userir_field2` and before the batch-3 `current_preset_data_request`; sent after it, its reply queues behind the ~1.2 s handshake drain.
  [→](../../notes/device/session.md#handshake)
- **Preset-list reassembly needs both stream rules:** `list_my_presets` tries `streams()` and `streams_final()` and keeps the longest decoded record set.
  [→](../../notes/device/presets.md#reading-presets)
- **`connect_for_discovery` waits out the handshake drain's `currentPresetDataChanged` (field 3)** (~2 s) for block enumeration, keeping OUT traffic flowing.
- **Boot-window `IOHIDDeviceSetReport failed: 0xe00002d6` is not an error**: the HID interface enumerates ~20 s before the device accepts reports.
  [→](../../notes/device/session.md#boot-window)
- **Serde casing exception (Copy):** `copy_apply`'s `CopyJob` / `CopyOp` / `CopyRepl` use camelCase nested keys because each field's explicit `#[serde(rename = "…")]` overrides the enum's `rename_all = "snake_case"`.
  Verify a wire shape against the per-field attributes, not the enum-level `rename_all`.

## Connection and state

- **Never wait silently for a reply.**
  The device drops the client after 750 ms with no host OUT report and discards the reply in flight, and a lapsed client ignores writes.
  Use `pump_collect_alive` / `send_and_collect_alive` / `pump_until_stable` for reply waits, and `pump_silent` only where the silence is the point (drains, `lapse`); `TrackedHid` reopens a lapsed client before any write.
  [→](../../notes/device/session.md#session-lapse)
- **Serialize every device operation** through `device_gate::DEVICE_OP_LOCK`, taken inside each device command's `spawn_blocking`; the seize is exclusive and two overlapping commands collide on `0xe00002c5`.
  [→](../../notes/device/session.md#exclusive-seize)
- **`connect_device` releases any old HID seize before enabling the monitor**; it does not acquire a UI-owned session.
  `list_presets` answers from the monitor's startup snapshot with no HID access or device-op lock.
  [→](../../notes/device/session.md#monitor-and-automatic-connection)
- **Scene reads are lazy, with no cache and no eager startup sweep.**
  [→](../../notes/device/presets.md#reading-presets)
- **Logging:** `Builder::new()` already ships the default `[Stdout, LogDir]` targets and `.target()` appends, so call `.clear_targets()` before re-adding or every line double-logs.
  Runtime code uses `log::*`; the `probe` / `gen_samples` CLIs keep `println!` because stdout is their interface.
- **Platforms:** `hid.rs` and `audio.rs` each carry one `imp` per platform (IOKit / hidraw / Win32 HID; CoreAudio / ALSA `hw:` / WASAPI), and nothing above that seam is platform-gated.
  `core-foundation-sys` is macOS-only (raw bindings; `core-foundation` is not a dependency), `libc` is Linux-only and `windows-sys` Windows-only.
  ALSA resolves the device through `/proc/asound`, and I32↔f32 conversion at the stream callback covers the integer-only `hw:` interface.
  Backup DBs are read in-process with bundled `rusqlite`; the `sqlite3` CLI is not a dependency.
  `watcher.rs` (hotplug) uses a non-seizing `IOHIDManager` on macOS and a 1 s presence poll on Linux and Windows (plus Configuration Manager notifications on Windows).
  Release and packaging facts are in [`notes/releasing.md`](../../notes/releasing.md); Windows development setup is in [`CONTRIBUTING.md`](../../CONTRIBUTING.md).
