# TMP Protocol Quick Reference

Snapshot of currently-known values across the TMP ecosystem.
Verify these are still accurate after firmware/app updates — the re-verify recipe at the end of this file tells you how to re-derive each one if it changes.

**Version baseline:** firmware 1.8.58 (the support floor); byte shapes captured from Pro Control v1.7.2.

## Preset format

- **XOR key**: `0x4A 0x4C 0x44` (ASCII `JLD`) — same operation encodes and decodes
- **Verify**: decoded result must be valid JSON starting with `{` and ending with `}`
- **If key changes**: re-derive the 3-byte XOR key by known-plaintext (the `{"` prefix) or Kasiski analysis (key length 3) + frequency analysis

## USB

- **VID**: `0x1ed8` (Fender)
- **PID**: `0x0044`
- **HID report size**: 64 bytes, vendor usage `0xFF00`

## Protocol Buffers

- **Top-level wrapper**: `FenderMessageTMS` with 13 category sub-messages.
- **Key messages**: `ChangeParameter`, `LoadScene` (PresetMessage field 101, addresses the current preset only; encoder golden-tested in `proto.rs`), `SetNodeSceneEdit`, `SetPresetLevel`, `SaveCurrentPreset`, `CurrentPresetDataRequest` (field 2 → the field-3 working copy).
- **Scene addressing**: `LoadScene.sceneSlot`, `SceneLoaded.sceneSlot` and `lastLoadedScene` are 0-based `scenes[]` indices; base is the constant wire slot 8, even for a preset with no scenes.
  The encoder must emit `sceneSlot` explicitly even for 0, because the device ignores an empty `LoadScene{}`.
  Which indices a preset accepts is [scene index bounds](../../../../notes/device/scenes.md#scene-index-bounds).
- **Firmware version**: `currentFwRequest` (SettingsMessage field 1, `dummy=true`, no `batchStatus`) → `currentFwResponse` (SettingsMessage field 2), `data` (field 1) = e.g. `"1.8.58"`; where it sits in the burst is [handshake](../../../../notes/device/session.md#handshake).
- **Working copy**: `currentPresetDataChanged` (PresetMessage field 3, one LZ4 block) answers `currentPresetDataRequest` (field 2) and is broadcast on every load, save or edit.
  `currentPresetDataJsonRequest` (field 78) is answered with field 79; the app does not send it.
  Field 136 is `AllBlockPresetsResponse` (the block-preset library, not preset data).
  Scene names and the live scene index are in field 3 (`scenes[].sceneName`, root `lastLoadedScene`, base = 8), and `sceneListResponse` (125) pushes the names on every preset load.
  The field-3 graph carries no slot: the active list index arrives in `PresetLoaded` (11), so the frontend's `activeListIndex` is the authoritative active slot.
  `probe --dump-currentpresetdata <slot>` loads a slot and dumps the decompressed JSON.
- **Stored-row read**: `presetDataRequest` (PresetMessage field 8, `{listEnum, slot}` 1-based, no `batchStatus`) → `presetDataChanged` (field 9, `presetJson` = field 3, plaintext, not LZ4).
  Keys are emitted byte-sorted at every level (`info` 8th of 15 top-level keys, `scenes` last); completeness and the live-session variant (`read_slot_preset_json_live`) are [reading presets](../../../../notes/device/presets.md#reading-presets).
- **Scene Edit**: `SetNodeSceneEdit { nodeId, groupId, sceneEditEnable: true }` (field 107), always answered by the broadcast `nodeSceneEditSet` (108).
  `nodeId` is the live `ChangeParameter` coordinate and may differ from the model's FenderId.
  Where a scene write lands is [scene-write landing](../../../../notes/device/scenes.md#scene-write-landing).
- **Proto count**: 379 files (Pro Control v1.7.2, SDK v3.13.0).

## USB HID wire envelope

```
host → device   (IOHIDDeviceSetReport, 63-byte payload, no report-id prefix):
    0x35 0x00 <body_len> <protobuf FenderMessageTMS>       padded to 63 with 0x00

device → host   (IOHIDDevice input-report callback, 64 bytes incl. report-id 0):
    0x00 0x35 0x00 <len> <body>            single-frame small response
    0x00 0x33 0x00 0x3c <60 bytes>         stream start (multi-packet response)
    0x00 0x34 0x00 0x3c <60 bytes>         stream continuation chunk
```

- **Host magic**: `0x35` for a small body; a body over 60 bytes goes out as `0x33` (start) / `0x34` (continuation) / `0x35` (final), each `MAGIC 0x00 <len> <≤60 B>`, as Pro Control does for `importPresetRequest`.
  **Device magic**: `0x35` / `0x33` / `0x34` (single, stream start, stream continuation).
- **Reassembler**: `0x33` begins a logical stream; append the 60-byte bodies of the following `0x34` chunks.
  `0x35` is the FINAL frame of that stream and carries payload; a `0x34` never follows a `0x35` within a stream.
  A reassembler that treats an inbound `0x35` as a standalone frame drops the last frame and truncates the tail of every multi-packet list.
- **Parse multi-packet responses fail-closed**: reject a body with a truncated length-delimited field (declared length > bytes present), accept only a response that parses complete, then retry the read.
  A tolerant parser clips a truncated field into a plausible but wrong record.
  Frames of two messages never interleave, so a short stream was cut [RE].

### `FenderMessageTMS` top-level fields

- 1 `testMessage`.
- 2 `presetMessage`: the dominant traffic.
- 3 `settingsMessage`.
- 4 `connectionMessage`: first connect lives here.
- 5–9 mixer, tuner, looper, backup, wifi.
- **10 `batchStatus` (int32)**, outside the oneof: a queue control for every message ([request batching](../../../../notes/device/session.md#request-batching)).
  The handshake sends `preset_list(1)=1`; favorite, `preset_list(4)`, `preset_list(3)`, product profile, current preset info, settings 66 and userir all `=2` (the in-burst `currentFwRequest` carries none); `current_preset_data_request=3` drains.
  Any later request: 3 or none; setters, commands and the heartbeat: none.
- 11–14 song, setlist, userIR, midi.

### First-connect handshake (Pro Control 1.7.2)

ConnectionRequest → PresetListRequest(listEnum=1) → FavoriteListRequest → PresetListRequest(listEnum=4) → PresetListRequest(listEnum=3) → ProductProfileRequest → CurrentPresetInfoRequest → SettingsMessage(field 66) → UserIRMessage(field 2) → CurrentPresetDataRequest, then an OUT report at least every 750 ms (the monitor heartbeats every 250 ms).
Its cost and the drain are [handshake](../../../../notes/device/session.md#handshake).

### Session messages

- **`ConnectionHeartbeat`** carries no `batchStatus`; its bytes are exactly `35 00 06 22 04 22 02 08 01`, and on a live session it draws no reply.
- **An empty `ConnectionError`** (`22 02 1a 00`, `errorEnum=0`) means the session is closed ([session lapse](../../../../notes/device/session.md#session-lapse)).
- `kIOReturnExclusiveAccess` is `0xe00002c5` and `kIOReturnNotPrivileged` is `0xe00002e2` ([exclusive seize](../../../../notes/device/session.md#exclusive-seize)).

#### Control-plane writes

- **Fire-and-forget setters and commands are sent without `batchStatus`**: `SetPresetLevel`, `LoadPreset`, `SaveCurrentPreset`, `ChangeParameter`, the Settings re-amp toggle.
- **Re-amp toggle** = `SettingsMessage(3) → reampModeActive(30)`: ON = `1a 05 f2 01 02 08 01`, OFF = `1a 03 f2 01 00`.
  Mixer `reAmpModeRequest(4)` → field 9 reads the real state; never send Mixer `setReAmpMode(15)` ([re-amp toggle](../../../../notes/device/reamp-audio.md#re-amp-toggle)).
- **`SetPresetLevel.presetLevel`** is a linear amplitude: captured loudness ≈ `20·log10(presetLevel) + C` ([`notes/leveling.md`](../../../../notes/leveling.md)).
- **`LoadPreset` uses field 6 = `presetSlot`**, not field 1.
  Field 1 is `tabEnum` (0=NotSet, 1=UserPresets, 2=FavoritePresets, 3=CloudPresets, 4=FactoryPresets, 5=Songs, 6=Setlists).
  The 1-based `presetSlot = list index + 1` rule also holds for the Factory bank (`tabEnum=4`) [HW].
  `presetListRequest{listEnum=4}` enumerates it.
- **`PresetListResponse.record[]`** is a `repeated PresetListRecord { string displayName }`: the slot is the 0-based array index, and no preset id is sent.
  The empty-slot marker is `--` (also "Empty"); every slot-addressed setter and `songPresetListResponse.userPresetSlot` is 1-based.
- **Preset CRUD setters** (byte-exact from a Pro Control capture; single-packet, no `batchStatus`): `renameCurrentPreset` (PresetMessage 13) `{displayName=f1}`; `moveUserPreset` (16) `{oldSlot=f1, newSlot=f2}`; `clearUserPreset` (15) `{userSlot=f1}`, a device slot.
  A Pro Control rename is `rename(13)` + `saveCurrentPreset(14){slot=f1}` ~10 ms later; rename alone does not persist.
- **`importPresetRequest`** (PresetMessage 117) `{presetJson=f1}`: `presetJson` = LZ4-block(raw `.preset` file bytes) = `LZ4(XOR_JLD(compact JSON))`, sent post-handshake as a chunked `0x33/0x34*/0x35` stream with no `batchStatus`.
  An all-literal LZ4 block is a valid trivial encoder.
  Where an import lands and what it answers is [importing](../../../../notes/device/presets.md#importing).
- **The `info` block on a save**: the firmware re-serializes `info` to exactly `{author, created_at, displayName, preset_id, product_id, source_id, timestamp, version}` on every `saveCurrentPreset`, minting a fresh `preset_id` [RE].
  Extra `info` keys survive import and restore (the blob is stored verbatim) but are dropped by the next save, so `presetJson` is no durable channel for host metadata ([preset identity](../../../../notes/device/presets.md#preset-identity)).
- **`exportPresetRequest`** (PresetMessage 115) `{listEnum=f1, presetSlot=f2}` → `exportPresetResponse` (116): the preset encrypted, then LZ4, so opaque without the device key.
  `requestNextEmptyPresetSlot` (81) is answered by a broadcast `nextEmptyPresetSlotResponse` (82).
- **Song-preset binding** (`assignSongPreset` / `songPresetListResponse` records, SongMessage field 11): the wire record names a preset by `userPresetSlot` (1-based device slot) + `presetSceneSlot` (0-based scene index, 8 = the whole preset); the binding itself is the preset's `UserPresets` row, which a move carries ([song links](../../../../notes/device/presets.md#song-links)).
  The Song read is `songPresetListRequest` 12 → response 13; refusals and link rules are in [`notes/songs-setlists.md`](../../../../notes/songs-setlists.md).
- **Song and setlist CRUD** (byte-exact from the device).
  Songs = `SongMessage` (TMS field 11): `addSong[4]{songName=1}`, `renameSong[7]{songSlot=1,songName=2}`, `removeSong[6]{songSlot=1}`, `songNotes[22]{songSlot=1,songNotes=2}`, `setSongBpmActive[23]{songSlot=1,active=2}`; list read `songListRequest[2]`→`songListResponse[3]`, record `{songName=1,songNotes=2,songBpmActive=3,songBpm=4}` (songName capped at 24 chars, accents folded to ASCII).
  Setlists = `SetlistMessage` (TMS field 12): `addSetlist[4]{setlistName=1}`, `renameSetlist[7]{setlistSlot=1,setlistName=2}`, `removeSetlist[6]{setlistSlot=1}`, `addSetlistSong[14]{setlistSlot=1,songSlot=2}`; list read `setlistListRequest[2]`→`setlistListResponse[3]` (record `{setlistName=1}`), membership `setlistSongListRequest[12]{setlistSlot=1}`→`setlistSongListResponse[13]` (records `{songSlot=1}`).
  `removeSetlistSong[16]{setlistSlot=1,setlistSongSlot=2}` and `moveSetlistSong[15]{setlistSlot=1,oldSetlistSongSlot=2,newSetlistSongSlot=3}` address the song by its 1-based POSITION in the setlist, not the global `songSlot` that `addSetlistSong` takes; `moveSetlistSong` is an array splice (`[1,6,11]` move 3→1 → `[11,1,6]`).
  Writes are fire-and-forget setters with no `batchStatus`; list reads use batch 3 or none; write slots are positional (1-based read-list index).
- **Per-song BPM** has no dedicated setter: `SettingsMessage[3].tapTempoBpm[12]{value(1)=float, originatorId(2)=1}` (byte-exact `1a09 6207 0d<f32le> 1001`) writes the active song's BPM, activated through `presetMessage[2].loadPreset[10]{tabEnum=5, songSlot=4, songPresetSlot=5, presetSlot=6}` (`LoadPreset` also has `setlistSlot=2`, `setlistSongSlot=3`).
  The required order is in [`notes/songs-setlists.md`](../../../../notes/songs-setlists.md).
- **`CurrentPresetDataChanged.presetJson`** (bytes field 1) is LZ4-block compressed JSON (not the LZ4 frame format: no `04 22 4d 18` magic, the length comes from the protobuf wrapper); its first byte `0xE8` is the first LZ4 token (`LL=14, ML=8`, the 14 literal bytes being the `{"ampControl":` that starts every preset).
  `AllBlockPresetsResponse.field1` (PresetMessage field 136) is also LZ4; `ProductProfileResponse.field1` (PresetMessage field 42) is plain pretty-printed UTF-8 JSON starting `{\n\t"effectCategories":`.
  Try LZ4 first, then fall back to a literal block.
- **The decoded `presetJson`** holds `audioGraph.guitarNodes.G1[]`, the ordered signal chain of blocks with `FenderId`, `dspUnitParameters`, `nodeId` and `nodeType`.
- **Pushes** need no subscription: every open session receives `PresetLoaded` (11), `SceneLoaded` (102), `CurrentPresetInfoChanged` (22), `sceneListResponse` (125) and field 3 on every change ([monitor](../../../../notes/device/session.md#monitor-and-automatic-connection)).

## Bluetooth LE

- **Service UUID**: `0c8d4c4d-27c4-4139-b052-be1bfb9a2ba3`
- **Write characteristic**: `e9dd73ec-...`
- **Notify characteristic**: `21c9812e-...`
- The app is USB-only and does not use BLE.

## Test & Loopback messages

TMS-native diagnostic / factory surface, all under `TestMessage` (the top-level oneof category, `TestMessage.proto`).
51 sub-types in total; the four most general-purpose are below.
The rest cover XMOS programming, SD card test, rear-panel pad / phantom / instrument-select / impedance switching, CPU temperature, brightness, MIDI output — see `TestMessage.proto:55-108` for the full catalog.

| Message | Request fields | Response | Use case |
|---|---|---|---|
| `LoopbackTest` | `string data = 1` | server echoes the same `LoopbackTest` back | generic HID round-trip parity / latency check |
| `MidiLoopbackRequest` | `bool dummy = 1` | `MidiLoopbackResponse { uint32 statusEnum = 1 }` | MIDI loopback hardware test (rear-panel MIDI in→out) |
| `SystemInfoRequest` | (no fields) | `SystemInfoResponse` | firmware/hardware version reporting |
| `CpuTemperatureRequest` | (no fields) | `CpuTemperatureResponse` | thermal telemetry — pair with high-load presets for parity tests |

All are `TestMessage`-routed (top-level oneof field 1); the request carries no `batchStatus` echo requirement.
None have a Mustang-LT-style `QASlots*` equivalent — TMS rolls factory tests under `TestMessage.testControl` / `testOutput` / `setTestAppMode` (`TestMessage.proto:59,58,100`) instead of an LT-style QA slot model.

See `references/protocol-lineage.md` for how the test surface compares to LT's `LoopbackTest` + `QASlots*` family.

## DSP

- **Effect count**: 311 (regenerate from the device's `product_profile.json`)

## How to verify after an update

```bash
# Pro Control version
defaults read "/Applications/Pro Control.app/Contents/Info.plist" CFBundleShortVersionString

# Re-verify the wire codec against the device's golden vectors
cd src-tauri && cargo test --lib proto

# Decode a known preset to check the XOR key
python3 .claude/skills/tmp-companion-protocol/scripts/preset_toolkit.py decode known.preset --pretty
```

If the decode output isn't JSON, re-derive the key by known-plaintext (the `{"` prefix) or Kasiski analysis (key length 3) — see **Preset format** above.
