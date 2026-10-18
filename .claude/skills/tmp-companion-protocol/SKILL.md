---
name: tmp-companion-protocol
description: "The USB wire-encoding and `.preset` codec reference for the Tone Master Pro: FenderMessageTMS field tables, HID framing and reassembly, the block-mutation message shapes, and the `.preset` XOR codec. Use whenever encoding or decoding a device message in `proto.rs` (codec + golden vectors), `session/` or `backup.rs`, adding a block-mutation op (insert/replace/remove) or a song/setlist setter, or decoding a `.preset` file. Device BEHAVIOUR (session lapse, batching, saves, scenes, re-amp) lives in `notes/device/`, not here."
---

# TMP Companion — wire encoding and preset codec

The TMP speaks Protocol Buffers (FenderMessageTMS) over USB HID, and its exported `.preset` files are XOR-only (the 3-byte JLD cipher) JSON; LZ4 wraps the file only inside `importPresetRequest.presetJson`.
This skill is the wire-ENCODING reference: field numbers, framing, message shapes and the codec.
What the device DOES with a message lives in `notes/device/`: [`session.md`](../../../notes/device/session.md) (seize, lapse, batching, handshake, reply queues), [`presets.md`](../../../notes/device/presets.md) (slots, reads, saves, imports, structural edits), [`scenes.md`](../../../notes/device/scenes.md) and [`reamp-audio.md`](../../../notes/device/reamp-audio.md).
The danger rules are [`.claude/rules/danger.md`](../../rules/danger.md).
The shipped implementation is `src-tauri/src/proto.rs` (byte-exact against golden vectors from the real device), `session/` and `backup.rs`.

## Wire envelope

- **Framing.**
  Outgoing frames are `0x35 0x00 <body_len> <FenderMessageTMS>`, padded; a body over 60 bytes goes out as `0x33` start / `0x34` continue / `0x35` final frames, each `MAGIC 00 LEN ≤60 B`.
  Inbound streams use the same markers, and `0x35` is the final frame of a stream, carrying payload.
  A stream that ends without its `0x35` final was cut and must not be parsed.
- **`batchStatus` is FenderMessageTMS field 10**, outside the oneof; setters, commands and the heartbeat are sent without it.
  Its queue semantics are [request batching](../../../notes/device/session.md#request-batching).
- **Slot addressing:** list reads are 0-based, slot-addressed setters 1-based; `session/preset.rs` owns the translation ([slot addressing](../../../notes/device/presets.md#slot-addressing)).
- Full field tables, the handshake order and the re-verify recipe are in [`references/quick-reference.md`](references/quick-reference.md).

## Block-mutation wire API

Live per-node structural edits drive `session::{replace_node,insert_node,remove_node}` and confirm on `nodeReplaced` (40) / `nodeInserted` (33) / `nodeRemoved` (36).
The exact shapes (`replaceNode` 39, `replaceNodeWithBlock` 100, `insertNode` 34 sent bare, `removeNode` 35, the `nodeJsonRequest` (119) edit-context preamble, 60-byte chunking) are in [`references/replacenode-family.md`](references/replacenode-family.md).
Confirms, node ids and the insert abort are [structural edits](../../../notes/device/presets.md#structural-edits) and [insert abort](../../../notes/device/presets.md#insert-abort).

## Preset `.preset` codec

`.preset` files are XOR-encoded compact JSON with the committed key `PRESET_XOR_KEY: [u8; 3] = *b"JLD"` (`src-tauri/src/backup.rs`); never reintroduce runtime key derivation.
The on-device DB (`normalDb.db3`, streamed by `read_library_via_backup`) stores `presetJson` as plaintext.
`currentPresetDataChanged` (field 3) carries one LZ4 block; `presetDataChanged` (field 9) carries plaintext.

`scripts/preset_toolkit.py` is a pure-Python JLD codec for debugging a `.preset`:

```bash
python3 .claude/skills/tmp-companion-protocol/scripts/preset_toolkit.py decode file.preset --pretty
python3 .claude/skills/tmp-companion-protocol/scripts/preset_toolkit.py inspect file.preset
python3 .claude/skills/tmp-companion-protocol/scripts/preset_toolkit.py diff a.preset b.preset
python3 .claude/skills/tmp-companion-protocol/scripts/preset_toolkit.py effects file.preset
```

## References

- [`references/quick-reference.md`](references/quick-reference.md): known values (XOR key, USB VID/PID, proto count), the top-level field table, framing and reassembly, the handshake order, the setter and CRUD message shapes, and the re-verify recipe.
- [`references/replacenode-family.md`](references/replacenode-family.md): the insert/replace/remove schemas and field numbers (drives `copyModel.ts` → `copy_apply` → `audiograph.rs`).
- [`references/protocol-lineage.md`](references/protocol-lineage.md): the public-sourced protocol heritage (Mustang LT → TMS), the prior-art provenance behind [`INTEROP.md`](../../../INTEROP.md).
