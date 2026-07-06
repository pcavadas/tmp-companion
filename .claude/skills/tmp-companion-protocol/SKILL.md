---
name: tmp-companion-protocol
description: "The USB wire-protocol + `.preset` codec reference for the Tone Master Pro. Use whenever working with the wire format in src-tauri/src/proto.rs (the FenderMessageTMS codec + golden vectors), session.rs (handshake + commands), backup.rs (the `.preset` XOR codec + library backup), or when adding/decoding a device message, a block-mutation op (insert/replace/remove), or a song/setlist setter. This is the wire-encoding reference — notes/protocol.md is the operational how-to (the load-bearing gotchas), CLAUDE.md is the architecture index. Bundled: references/quick-reference.md (the field tables + framing + re-verify recipe), references/replacenode-family.md (the block-mutation wire shapes), references/protocol-lineage.md (the public Mustang-LT→TMS heritage), scripts/preset_toolkit.py (decode/encode/inspect a `.preset`)."
---

# TMP Companion — wire protocol & preset codec

The TMP speaks **Protocol Buffers (FenderMessageTMS) over USB HID**, and its exported presets are XOR+LZ4-encoded JSON. This skill is the wire-format _reference_; the operational rules (what wedges the device, batch grouping, slot addressing, re-amp latching) live in **`notes/protocol.md`** and **`CLAUDE.md`**'s Gotchas — when this skill and `CLAUDE.md` disagree, `CLAUDE.md` wins. The shipped implementation is `src-tauri/src/proto.rs` (byte-exact vs golden vectors captured from the real device), `session.rs`, and `backup.rs`.

## How to verify a wire claim

- **Codec claim** (encoding, field number, framing) → a golden-vector test in `proto.rs` (`cd src-tauri && cargo test --lib`); add a vector when you add a message.
- **Live-behavior claim** (what the device answers, latch/order rules) → re-verify with a non-destructive `probe` subcommand against the real unit; never extrapolate from the codec.
- **Never trust a device ack alone** — echoes are flaky (`ReAmpModeChanged`) and edits can silently no-op; verify by observed effect (finite captured loudness, a read-back, the specific `nodeXxx` confirm).

## Wire envelope (the load-bearing bytes)

- **Framing.** Outgoing frames are `0x35 0x00 <body_len> <FenderMessageTMS>` padded; multi-packet outbound is `0x33` start / `0x34` continue / `0x35` final, each `MAGIC 00 LEN ≤60B`. Inbound streams use the same `0x33/0x34/0x35` markers (0x35 = single-frame / final). Reassembly must handle both an interleaved-`0x35` mid-flood and a terminal `0x35` tail frame.
- **`batchStatus` (FenderMessageTMS field 10) is a GROUP correlator, not a per-request counter.** The host **reuses** one value across a group of related requests (mirroring Pro Control's handshake groups). Incrementing it per request makes the device answer the first couple then go silent. **Setters/commands + the heartbeat OMIT `batchStatus` entirely** — a setter sent _with_ one is silently ignored.
- **Slot addressing:** `list_my_presets` is 0-based; the slot-addressed setters (`loadPreset`/`saveCurrentPreset`/`clearUserPreset`/`moveUserPreset`) are **1-based** — device `userSlot = list index + 1`. `session.rs` owns the translation; callers pass 0-based list indices.
- **The handshake is load-bearing** — the device only answers after the captured first-connect sequence (`connection_request` + the preset lists + product profile + current-preset/settings/userir requests). Full field tables, message hierarchy, and the re-verify recipe: **`references/quick-reference.md`**.

## Block-mutation wire API

Live per-node structural edits (insert / replace / remove a signal-chain block) drive `session::{replace_node,insert_node,remove_node}` and confirm on `nodeReplaced(40)` / `nodeInserted(33)` / `nodeRemoved(36)`; a `presetError(53)` or an unconfirmed edit must **never** be saved. The exact wire shapes (`replaceNode` 39, `replaceNodeWithBlock` 100, `insertNode` 34 sent bare, `removeNode` 35, the `nodeJsonRequest`(119) edit-context preamble, 60-byte chunking) are in **`references/replacenode-family.md`**. The write-safety rules around these ops (the three keyed places, exact-FenderId matching) live in `notes/write-safety.md` + `notes/block-copy.md`.

## Preset `.preset` codec

`.preset` files are **XOR-encoded compact JSON** (the 3-byte JLD cipher — LZ4 is NOT applied to the file itself). The 3-byte XOR key is a committed constant: `PRESET_XOR_KEY: [u8; 3] = *b"JLD"` in `src-tauri/src/backup.rs` (do NOT reintroduce runtime key derivation/recovery). The on-device DB (`normalDb.db3`, streamed via the `BackupMessage` bulk path — `read_library_via_backup`) stores `presetJson` as **plaintext**; the exported `.preset` file XOR-encodes it (LZ4 wraps the raw `.preset` bytes only inside the `importPresetRequest.presetJson` wire field, not the file). The offline `.preset` is the canonical full-preset read (USB reads return a partial).

`scripts/preset_toolkit.py` is a pure-Python JLD codec for debugging a `.preset` while working a write path:

```bash
python3 .claude/skills/tmp-companion-protocol/scripts/preset_toolkit.py decode file.preset --pretty
python3 .claude/skills/tmp-companion-protocol/scripts/preset_toolkit.py inspect file.preset
python3 .claude/skills/tmp-companion-protocol/scripts/preset_toolkit.py diff a.preset b.preset
python3 .claude/skills/tmp-companion-protocol/scripts/preset_toolkit.py effects file.preset
```

## References

- **`references/quick-reference.md`** — the current known values (XOR key, USB VID/PID, proto count, key message types), the field tables, the framing/reassembly rules, the handshake grouping, and the recipe to re-verify them after a firmware/app update.
- **`references/replacenode-family.md`** — the insert/replace/remove wire schemas + field numbers + the confirm/never-save-on-error gate (drives `copyModel.ts` → `copy_apply` and `audiograph.rs`).
- **`references/protocol-lineage.md`** — the public-sourced protocol heritage (Mustang LT → TMS via Fender's tone SDK): inherited framing / single-in-flight / heartbeat, TMS-added 13-way oneof router + `UndoReplaceNode` + LZ4 `presetJson`. Prior-art provenance for the interoperability posture (see `INTEROP.md`).
