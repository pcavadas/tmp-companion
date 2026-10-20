---
paths:
  - "src-tauri/src/commands/copy_apply.rs"
  - "src-tauri/src/commands/bulk_replace.rs"
  - "src-tauri/src/commands/presets.rs"
  - "src-tauri/src/replace_inplace.rs"
  - "src-tauri/src/slot*.rs"
  - "src-tauri/src/backup*.rs"
  - "src-tauri/src/blockcaps.rs"
  - "src-tauri/src/audiograph.rs"
  - "src-tauri/src/bulkrun.rs"
  - "src-tauri/src/preset_io.rs"
  - "src-tauri/src/library.rs"
  - "src-tauri/src/paramedit.rs"
  - "src-tauri/src/commands/held_edit.rs"
  - "src-tauri/src/e2e_seed.rs"
  - "src-tauri/src/param_class.rs"
---

# Preset write rules

Read [`notes/device/presets.md`](../../notes/device/presets.md) before changing a read, load, save or import, and [`notes/device/structural-edits.md`](../../notes/device/structural-edits.md) before changing a structural edit or block-cap check.
Its sections are the authority for slot addressing, complete reads, the save and dirty-flag rules, rejected-at-load bodies, preset identity, parameter ranges, footswitch JSON, structural-edit confirms, node ids and the insert abort.
`blockcaps.rs` is the only enforcement of the block caps, so every apply path calls it ([insert abort](../../notes/device/structural-edits.md#insert-abort)).
The block-copy feature model is [`notes/copy.md`](../../notes/copy.md); the `.preset` codec and the block-mutation wire shapes are the `tmp-companion-protocol` skill; the danger rules are in [`danger.md`](danger.md).
