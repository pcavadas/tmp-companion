# Protocol lineage — Mustang LT → TMS

TMP's USB protocol is not an independent design. It is a direct lineal descendant of the protocol Fender introduced for the **Mustang LT** family (and shipped under the internal `tone-sdk` C++ library after the 2018-era app rewrite). This reference captures what TMS inherited verbatim, what it added, and what it dropped. It exists so a future session does not need to re-derive the lineage from scratch.

## The evidence chain

- [LtAmp by brentmaxwell](https://github.com/brentmaxwell/LtAmp) — open-source .NET library for Mustang LT. Ships [`Docs/Protocol.md`](https://github.com/brentmaxwell/LtAmp/blob/main/Docs/Protocol.md) (wire envelope) and `Schema/protobuf/` (full message catalog).
- [Hayden Bursk, "C++ Desktop Application Architecture for Digital Amplifier Connectivity", Fender Engineering on Medium](https://medium.com/fender-engineering/c-desktop-application-architecture-for-digital-amplifier-connectivity-454da5c44026) — first-party write-up of the `tone-sdk` SDK design that underpins the LT desktop app (and, by structural identity, TMS).
- [fmmp by spod](https://github.com/spod/fmmp) — Mustang Micro Plus recon. Has `protobuf/{gt,ltx}/` subtrees and documents the wider Fender connected-device protocol family.

## Inherited from LT (identical or near-identical)

| Aspect                    | Mustang LT (LtAmp Protocol.md)                                                          | TMP TMS (this repo)                                                                                                                                                                             | Notes                                                                         |
| ------------------------- | --------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| HID packet size           | 64 B                                                                                    | 63–64 B (`quick-reference.md` "USB HID wire envelope")                                                                                                                                          | one-byte cosmetic difference in report-id handling                            |
| Frame tags                | `0x33` stream-start, `0x34` continuation, `0x35` final/single                           | same trio (`quick-reference.md` line 34-36)                                                                                                                                                     | byte values identical                                                         |
| Host→device layout        | `[tag\|len\|value(60)\|pad]`                                                            | `[0x35\|0x00\|len\|protobuf(63)]`                                                                                                                                                               | same shape, TMS uses one trailing slot of the report for header               |
| Wrapper protobuf          | `FenderMessageLT` with `responseType` + sub-message                                     | `FenderMessageTMS` with `oneof type` (13-way) + `batchStatus`                                                                                                                                   | same envelope idea, TMS uses proto3 oneof instead of LT's tagged responseType |
| Heartbeat                 | mandatory every 1 s (`ModalStatusMessage` per LT spec)                                  | `ConnectionHeartbeat`, ~1.5 s, body `bool dummy = 1` (`ConnectionHeartbeat.proto`)                                                                                                              | same model                                                                    |
| Single-in-flight contract | enforced at `tone-sdk` "Send thread" (Medium post)                                      | TMS encodes via `batchStatus int32 = 10` outside the oneof (`FenderMessageTMS.proto:18`); host REUSES one value per group of related requests (NOT a per-request counter — see quick-reference) | same intent, different mechanism                                              |
| Source-of-truth           | "hardware is single source of truth, app is companion" (Medium post)                    | TMP polls device on touchscreen-driven changes; no unsolicited `CurrentPresetDataChanged` (`quick-reference.md:65`)                                                                             | same philosophy                                                               |
| Echo prevention           | early LT app filtered `SetParameter` echoes; later firmware disabled echo (Medium post) | TMS untested at this layer — assume same behaviour until proven otherwise                                                                                                                       | flag for future drivers that mutate state                                     |

## Added in TMS (no LT equivalent)

- **13-way `oneof type` router** in `FenderMessageTMS` (`FenderMessageTMS.proto:19-33`): TestMessage / PresetMessage / SettingsMessage / ConnectionMessage / MixerMessage / TunerMessage / LooperMessage / BackupMessage / WifiMessage / SongMessage / SetlistMessage / UserIRMessage / MidiMessage. LT has a single flat `FenderMessageLT` with no category dispatch.
- **`UndoReplaceNode` + `UndoReplaceNodeResponse`** (`UndoReplaceNode.proto`, `UndoReplaceNodeResponse.proto`, surfaced via `PresetMessage.proto:113-114, 260-261`): LT has `ReplaceNode` + `ReplaceNodeStatus` but no undo. TMS's undo is **stateless** — the caller carries `originalNodeJson` payload; no server-side undo stack. See `replacenode-family.md`.
- **LZ4-block-compressed `presetJson` payload** inside `CurrentPresetDataChanged.presetJson` and `AllBlockPresetsResponse.field1` (`quick-reference.md:63`). LT exchanges presets as plain JSON.
- **`SongMessage` + `SetlistMessage` + `UserIRMessage`** categories in the top-level oneof — three product surfaces LT does not have (no song player, no setlist mode, no user IR import on the LT).
- **Rich factory/QA test surface** in `TestMessage` (51 sub-types: XMOS programming, SD card test, brightness, CPU temperature, rear-panel pad/phantom/instrument-select/impedance switching). LT exposes `QASlots*` + `LoopbackTest` only. See the Test/Loopback subsection of `quick-reference.md`.
- **Scene/scene-edit surface** in `PresetMessage` (`LoadScene`/`CreateScene`/`DeleteScene`/`SetNodeSceneEdit`/`SetSceneAmpControl{1,2}`, fields 101-114 and 133-134). LT presets are flat; scenes are a TMS-era addition.

## Absent in TMS (mechanism is open)

These message families appear in LT but are reproducibly absent from TMS's extracted proto tree (grep the extracted `*.proto` returns zero hits for each):

- **`ModalStatusMessage`** with `SYNC_BEGIN` / `SYNC_END` context wrappers. LT uses these to bracket bulk-sync sessions; TMS does not. TMS's first-connect handshake (`quick-reference.md:53-55`) is request/response only, no bracket. **Open question:** does TMS bracket its batches some other way, or is `batchStatus` field 10 (the LT-absent correlator) doing the equivalent job?
- **`AuditionPreset*`** family (preview-without-commit). Pro Control has a UX-level cloud-preset preview surface, so the _capability_ exists — but no `Audition`-named message carries it. Possibly fulfilled by `LoadPresetDetached` (`PresetMessage.proto:219`, field 80) which has no LT analogue and whose name suggests "load preset without committing it to current slot". **Open question, do not assume.**
- **`FrameBufferMessage`**. LT can request the amp's display as a framebuffer for app-side mirroring. TMS has a 1024×600 touchscreen; if Pro Control ever mirrored it the message would exist. The absence is itself a finding: Pro Control rebuilds the UI from the preset JSON, it does not framebuffer-mirror.
- **`ProcessorUtilization`** / `MemoryUsage`. LT exposes CPU/MEM telemetry. The TMP manual surfaces a "Block Limit" CPU-exhaustion warning, so the firmware tracks the same data — but no proto carries it on the wire. **Open question:** is this surfaced via SettingsMessage / TestMessage / a yet-unmapped category, or is it pure firmware-internal state?

These four gaps are **not investigated** in this reference; they are catalogued as starting points for a future session.

## SDK architecture (from the Medium post)

The Medium post documents `tone-sdk` as a separate C++ library with:

- **Four-thread model:** UI / Send / Model / HID. Events flow UI → send queue → HID → response/ack queue → model queue → RxCpp observables → UI.
- **Lock-free queues:** initially `moodycamel::ConcurrentQueue`, replaced with `boost::lockfree::queue` after discovering moodycamel is not linearizable across multi-producer scenarios (caused backup/restore message interleaving in LT testing).
- **Pattern-match dispatch:** `ni::matchine` (Native Instruments' open-source pattern-matching library) for handling responses.
- **Observer pattern:** `RxCpp` (Reactive Extensions for C++), exposed via a `ModelObservable` wrapper. Two subject flavours: `Rx::behavior<T>` (replays current state on subscribe) and `Rx::subject<T>` (notifies future changes only).
- **Transport abstraction:** HID is one transport; the post explicitly notes "if the transport mechanism was to change, to BLE for example, we would only need a new transport layer."

These are architectural priors.

## What this changes about TMP recon

- The `0x33 / 0x34 / 0x35` framing is **not a TMP innovation**; it has been in the public domain since the LT release. Recon docs may safely cite "the Fender `tone-sdk` HID envelope" as prior art.
- Naming convention `FenderMessage{LT,GT,LTX,TMS}` is a family-naming pattern. Future Fender connected products are likely to use `FenderMessage<two-or-three-letter-product-tag>`.
- The single-in-flight contract is an SDK-level invariant; any TMS driver that pipelines requests without waiting for the matching `batchStatus` echo is fighting the firmware's design.
- The presence of `UndoReplaceNode` and the absence of `ModalStatus` brackets suggest TMS evolved toward **stateless / per-call atomicity** rather than LT's session-bracket model. Drivers should mirror this: one message, one effect, no implicit session state.

## Sources

- [LtAmp Protocol.md](https://github.com/brentmaxwell/LtAmp/blob/main/Docs/Protocol.md) — packet format, tags, handshake, message catalog
- [LtAmp protobuf schemas](https://github.com/brentmaxwell/LtAmp/tree/main/Schema/protobuf) — `FenderMessageLT.proto`, `ModalStatusMessage.proto`, `AuditionPreset.proto`, `FrameBufferMessage.proto`, `ProcessorUtilization.proto`, `MemoryUsageRequest.proto`, `ReplaceNode.proto`
- [Fender Engineering Medium: C++ Desktop App Architecture](https://medium.com/fender-engineering/c-desktop-application-architecture-for-digital-amplifier-connectivity-454da5c44026) — `tone-sdk` design
- [fmmp protobuf/gt](https://github.com/spod/fmmp/tree/main/protobuf/gt) — Mustang GT cousin, confirms the family
- TMS source-of-truth: the extracted client-app protobuf (`FenderMessageTMS.proto`, `PresetMessage.proto`, `TestMessage.proto`, `ConnectionMessage.proto`, `ConnectionHeartbeat.proto`; mirrored in the companion's `src-tauri/src/proto.rs` golden vectors) + `quick-reference.md`
