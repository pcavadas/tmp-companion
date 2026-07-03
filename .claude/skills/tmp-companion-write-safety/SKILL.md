---
name: tmp-companion-write-safety
description: "The data-loss-prevention checklist for any code path that WRITES to the Tone Master Pro — saving a leveled preset, editing/adding/removing a signal-chain block, copying blocks between presets, clearing/moving a slot, or any live changeParameter+save. Use this skill before touching audiograph.rs, preset_io.rs, bulk_cmd.rs, session.rs write setters, copy_apply, or src/views/copy/copyModel.ts. The device holds irreplaceable user presets and there is no reliable full-preset read-back, so a missing guard corrupts or destroys real data. Read notes/write-safety.md and notes/block-copy.md for the full reasoning; this is the pre-flight checklist."
---

# TMP Companion write safety

Writes land on real hardware holding presets the user cannot re-download. `notes/write-safety.md` and `notes/block-copy.md` are the reference bodies (the _why_); this skill is the pre-flight checklist (the _what to never skip_). `CLAUDE.md`'s Gotchas are the authoritative running log — when they disagree, `CLAUDE.md` wins.

## Before any write, confirm

1. **Never save on an unconfirmed edit.** Do not `save_current_preset` after a `presetError(53)` or before a structural edit is confirmed on `nodeReplaced(40)` / `nodeRemoved(36)` / `nodeInserted(33)`. A wrong-content save has corrupted a real slot. The confirm gate is the whole point of the held-session machinery — don't shortcut it.

2. **A block lives in THREE differently-keyed places — touch all or leave dangling state.** (a) the roster `audioGraph.{guitarNodes,micNodes}` (by FenderId), (b) per-scene overrides `scenes[].<group>.<FenderId>` (by FenderId), (c) footswitch assignments `ftsw[i][].nodes[].nodeId` (by **exact** nodeId). Miss (b) → stale inert overrides; miss (c) → a footswitch ghost-toggles a block that's gone. Use the shared `drop_scene_overrides` / `retarget_ftsw` helpers (`audiograph.rs`) for remove/replace/create-variant.

3. **The backend matches EXACT FenderId.** The `ConvRvb|CabIR|…` suffix-normalization (`resolveDeviceId`) is **frontend-only**; a frontend-normalized id passed into a backend `OpSpec` silently fail-matches (no error, no replacement). Pass raw device ids into ops.

4. **Guard destructive slot ops in the mutation's own address space.** Before `clear` / `move` / `save`-over keyed on a slot mapping, confirm the mapping with a **non-destructive read first**, and put the guard in the **same** space as the mutation: device `userSlot = list index + 1` (`session.rs` owns the translation; callers pass 0-based list indices). A guard checked in list-index space while `clear` acted in 1-based slot space once deleted a real preset.

5. **Preserve preset identity on in-place edits.** Overwriting a song-bound slot with a different-identity preset empties the Song row. Link-safe editing preserves `info.preset_id` + scene structure (live `changeParameter`+save, or an identity-preserving offline re-import).

6. **The offline `.preset` is the canonical full read.** USB reads return a device-truncated partial; write paths that need the complete preset read the offline `.preset` file, not a USB round-trip.

## Where things live

`src-tauri/src/`: `audiograph.rs` (node ops + the 3-keyed-place helpers), `preset_io.rs`, `bulk_cmd.rs`, `session.rs` (slot translation + write setters), the `copy_apply` command. Frontend: `src/views/copy/copyModel.ts` (edit→op diff). Reference docs: `notes/write-safety.md`, `notes/block-copy.md`.
