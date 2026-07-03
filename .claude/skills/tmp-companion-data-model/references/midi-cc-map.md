# MIDI CC Map — TMP firmware v1.7

Reproduced from the MIDI Implementation Chart in the Owner's Manual (p.43), plus surrounding semantic notes from pp.42-43.

## Contents

- [Bank Select + Program Change](#bank-select--program-change)
- [Control Change (CC) — Full Table](#control-change-cc--full-table)
- [MIDI Out jack modes](#midi-out-jack-modes)
- [Receive channel & filtering](#receive-channel--filtering)
- [MIDI Clock](#midi-clock)
- [Gotchas](#gotchas)

## Bank Select + Program Change

504 presets across 4 banks — banks 0–2 hold 128 each, bank 3 holds 120 (see the table below). Each preset is recalled by sending a Bank Select (CC 0) followed by a Program Change.

| Preset range | Bank Select CC 0 value | PC#   |
| ------------ | ---------------------- | ----- |
| 1–128        | 0                      | 1–128 |
| 129–256      | 1                      | 1–128 |
| 257–384      | 2                      | 1–128 |
| 385–504      | 3                      | 1–120 |

(PC values are 1-indexed per Fender's table — the wire value is `PC# - 1` for standard MIDI implementations.)

## Control Change (CC) — Full Table

From OM p.43.

| CC # | Value range                               | Function                                                |
| ---- | ----------------------------------------- | ------------------------------------------------------- |
| 0    | 0–3                                       | Bank Change (= Bank Select MSB)                         |
| 1    | 0–127                                     | Expression Pedal 1                                      |
| 2    | 0–127                                     | Expression Pedal 2                                      |
| 3    | 0–127                                     | **MIDI Expression Pedal 3** (virtual; no physical jack) |
| 4    | 0–127                                     | **MIDI Expression Pedal 4** (virtual; no physical jack) |
| 7    | 0–127                                     | Master Volume                                           |
| 20   | 0-63 OFF, 64-127 ON                       | FS Mode Enable                                          |
| 21   | 0-63 OFF, 64-127 ON                       | Effects Footswitch 1                                    |
| 22   | 0-63 OFF, 64-127 ON                       | Effects Footswitch 2                                    |
| 23   | 0-63 OFF, 64-127 ON                       | Effects Footswitch 3                                    |
| 24   | 0-63 OFF, 64-127 ON                       | Effects Footswitch 4                                    |
| 26   | 0-63 OFF, 64-127 ON                       | Effects Footswitch 5                                    |
| 27   | 0-63 OFF, 64-127 ON                       | Effects Footswitch 6                                    |
| 28   | 0-63 OFF, 64-127 ON                       | Effects Footswitch 7                                    |
| 29   | 0-63 OFF, 64-127 ON                       | Effects Footswitch 8                                    |
| 30   | 0-7 enables FS 1-8; 10-17 disables FS 1-8 | Effects Footswitch Selection (bulk enable/disable)      |
| 64   | 64–127                                    | Tap Tempo                                               |
| 65   | 0-63 OFF, 64-127 ON                       | Toe Switch                                              |
| 66   | 0-63 OFF, 64-127 ON                       | Amp Control 1 (tip of AMP CTRL TRS jack)                |
| 67   | 0-63 OFF, 64-127 ON                       | Amp Control 2 (ring of AMP CTRL TRS jack)               |
| 68   | 0-63 OFF, 64-127 ON                       | Tuner (toggle on/off)                                   |
| 69   | 64–127                                    | Next Song                                               |
| 70   | 64–127                                    | Previous Song                                           |
| 103  | 64–127                                    | Looper REC/DUB                                          |
| 104  | 64–127                                    | Looper PLAY/STOP                                        |
| 105  | 64–127                                    | Looper 1-SHOT                                           |
| 106  | 64–127                                    | Looper UNDO                                             |
| 107  | 64–127                                    | Looper 1/2 SPEED                                        |
| 108  | 64–127                                    | Looper REVERSE                                          |
| 109  | 64–127                                    | Looper VOLUME UP                                        |
| 110  | 64–127                                    | Looper VOLUME DOWN                                      |

CC 25 is intentionally skipped (no Effects Footswitch 5 at CC 25 in the chart — Fender uses CC 26 for FS 5).

## MIDI Out jack modes

Configurable in Global Settings → I/O → MIDI → "MIDI OUT":

| Mode            | Behavior                                                                               |
| --------------- | -------------------------------------------------------------------------------------- |
| `Out` (default) | Only MIDI messages **generated by TMP** are sent to the MIDI OUT jack                  |
| `Thru`          | Only MIDI messages **received at MIDI IN** are sent to the MIDI OUT jack (passthrough) |
| `Merge`         | Both received and generated messages are merged onto the MIDI OUT jack                 |

## Receive channel & filtering

Global Settings → I/O → MIDI:

- **Receive Channel**: 1–16 or Omni (default Omni). TMP responds to MIDI commands only on the selected channel (or all channels if Omni).
- **Receive MIDI PC**: enable/disable receiving Program Change messages, per transport: `MIDI` only / `USB` only / `MIDI + USB` (default).
- **Receive MIDI CC**: same per-transport toggle for Control Change messages.
- **Receive MIDI Clock**: same per-transport toggle for MIDI Clock (default OFF).
- **Rename MIDI Channels**: outgoing channels can carry user-supplied labels stored in prefs. These labels appear in any preset/scene MIDI workflow UI on the touchscreen.

## MIDI Clock

- **Send MIDI Clock**: Global Settings → I/O → MIDI → "Send MIDI Clock". Routes clock to MIDI OUT jack and/or USB-C port. Default OFF.
- **Send MIDI PC/CC**: routes generated PC/CC messages to MIDI OUT and/or USB-C. Default `MIDI + USB`.
- **Receive MIDI Clock**: when ON, TMP slaves its tempo to incoming Clock. All delay and modulation effects with an assigned Tap Division respond to incoming Clock.

## Gotchas

1. **Receive Clock disables tap footswitch.** When `Receive MIDI Clock` is ON, the per-preset saved tempo is disregarded, the tap footswitch is locked out, and the tempo displayed in the Tap scribble strip + preset view reflects the externally received Clock.

2. **MIDI EXP 3 and MIDI EXP 4 have no physical jack.** They only exist on the wire. To use them, you must send CC 3 / CC 4 from an external MIDI controller (e.g. another floor controller, a DAW automation lane, or a MIDI-over-USB script).

3. **CC 30 is a bulk control.** Value 0 enables FS 1, 1 enables FS 2, ..., 7 enables FS 8. Values 10..17 disable the corresponding FS. Values outside these ranges are ignored.

4. **CC 25 is skipped** between FS 4 (CC 24) and FS 5 (CC 26). The convention is asymmetric — don't assume contiguous CC# assignment.

5. **AMP CTRL 1 and 2 are tip/ring of a single TRS jack.** Use a TRS-to-dual-TS insert cable to wire them to two separate amp control inputs (e.g. amp channel select + reverb toggle).

6. **Bank Select changes do not take effect until a PC arrives.** Standard MIDI bank-select behavior. Send Bank Select first, then PC.

## Source

Fender Tone Master Pro Owner's Manual, firmware v1.7, pp. 42-43 (MIDI section + MIDI Implementation Chart). Re-fetch from `www.fender.com/tonemaster_pro` when firmware revs (the CC map has been extended in past firmware releases).
