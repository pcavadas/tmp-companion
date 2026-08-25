# What the manual does NOT answer

Source: Owner's Manual v1.8 (rev. J), read end to end (text layer + every page rendered as an image). Page refs are **printed** pages.

**Purpose.** Every entry here is a question someone will ask that the manual genuinely does not settle. Recording them once stops the same searches being repeated, and — more importantly — stops an _inference_ being quoted back later as a documented fact.

Where one of these unknowns bears on companion code specifically, the engineering-side write-up is `notes/device-manual-gaps.md` — that file tracks the probes and gets entries deleted as they close; this file tracks what the _manual_ does not say and is stable until Fender revises it.

Three tags are used:

- **UNDOCUMENTED** — the manual is silent. Needs a hardware probe or firmware inspection.
- **AMBIGUOUS** — the manual says two things, or one thing that admits multiple readings.
- **INFERRED** — the reference states a conclusion the manual supports but does not assert. Safe to use, must stay labelled.

Two settled states also appear, and the distinction between them is load-bearing:

- **RESOLVED (HW-derived)** — settled by **measurement on a real unit**. The strongest evidence there is.
- **RESOLVED (FW-derived, static)** — settled by **reading the firmware** (binary strings, Qt MOC tables, embedded proto descriptors, preset JSON in a backup). No unit involved.

Keep them apart. A static read proves what the firmware _contains_, never what the device _does_ — A2 exists precisely because a plausible static/manual claim (48 kHz end to end) was overturned by measurement. A FW-derived entry is a strong lead and a safe default; it is not a substitute for a capture.

Every settled entry cites the firmware version it was measured on (**1.8.45** throughout, unless noted), since some of these facts could change on a firmware revision.

> **Measurement provenance.** Every LUFS figure here was reproduced exactly by an independent BS.1770-4 implementation from the raw WAVs. The probe was built **without `--features e2e`** (that feature fabricates all LUFS) — verified by build-log audit and by `strings` on the binary. Device integrity is checked by SHA-256 over each `presetJson` against the session-start backup.
>
> Three standing cautions for anyone taking new captures:
>
> - Captures use `loudest_channel()`, which argmaxes RMS over the **processed pair only** — `(0..channels.min(2))` in `audio.rs`, so channel 2's dry instrument send can never win the argmax. What is still an assumption rather than a recorded fact is **which of the two processed lanes** a given figure came from: on a genuinely stereo preset L and R can trade loudest between runs, flipping any per-channel verdict with them. Prefer `stereo_mix()` (the deterministic 0/1 average) for lane work.
> - The LUFS figures below are **mono-era**: they predate the stereo re-baseline and were measured as a single channel, so they read ~3.01 dB below what the app now reports for the same capture (`lufs::measure_stereo` over the processed pair — see `lufs.rs`'s module header). The comparisons and deltas are unaffected; only the absolute numbers moved. That fixed +3.0103 dB offset (HW-measured on fw 1.8.45) holds only for the **mirrored dual-mono** processed pair — a genuinely stereo preset whose two processed lanes differ has no fixed offset between a single-channel figure and the 2-ch BS.1770 value, so such figures need remeasurement, not conversion.
> - Check headroom. `h_noG2.wav` peaks at exactly 1.000000; immaterial to its integrated LUFS, but its own >22 kHz rows are clipping products, not signal.

---

## A. Load-bearing for measurement and leveling

### A1. Does the master volume knob affect the USB 1/2 output? — **RESOLVED: NO (HW-derived, fw 1.8.45)**

**Measured: the master volume knob does NOT affect USB 1/2.** Moving it 50 % → 80 % changes the USB 1/2 capture by **0.001 LU** — the measurement floor. Anything measuring USB 1/2 is therefore _not_ riding a hidden master-knob offset, and the p.3 wording ("volume for **all outputs**") does not extend to this channel.

| condition                                   | captures (integrated LUFS)         | Δ vs baseline           |
| ------------------------------------------- | ---------------------------------- | ----------------------- |
| baseline, master 50 %                       | −24.050, −24.050, −24.049, −24.050 | — (spread **0.001 LU**) |
| **master 80 %**                             | −24.049, −24.050, −24.050          | **0.001 LU — nothing**  |
| **USB 1/2 fader −10 dB** (positive control) | −30.530, −30.530, −30.530          | **−6.48 LU**            |

**The positive control is what makes the null trustworthy.** A null on its own is ambiguous — it cannot distinguish "the knob is out of the path" from "this capture is blind to the mixer entirely". Pulling the **USB 1/2 channel fader** (the very channel being measured) moved the capture by 6.48 LU, ~6 500× the noise floor. The tap demonstrably sees that channel; the knob simply produces nothing on it.

> **Do not read the fader row as a dB calibration.** A fader marked −10 dB produced −6.48 LU, not −10. That gap is unexplained — candidates include a non-linear fader taper, the marking not being a pure output gain, and K-weighted integrated loudness not tracking broadband gain 1:1 on this signal. **Not measured, do not quote −10 dB ⇒ −6.48 LU as a transfer function.**

**Mechanism, observed directly on the unit.** Moving the master knob visibly moves the **Headphones + Output 1 + Output 2** faders together, because those three carry the `MASTER` flag — and `USB 1/2` does not. That is the same split recorded in the backup (`linkToMasterLvl: true` on exactly those three, `false` on `usb12`), so the flag has real audio consequences rather than being a UI decoration. The manual states the flag's existence (p.36) and the knob's scope (p.3) in two places that appear to contradict; the flag is what reconciles them.

**Consequence for leveling:** the master knob may be left anywhere without biasing a USB 1/2 measurement. The **USB 1/2 channel fader, mute and solo** are a different matter — the fader alone moves the tap by many LU, and mute/solo/AUX are separately measured below and confirmed audio-relevant too — so the pre-flight described below is still required. See also **B0**: the output-assign matrix is rewritten on every preset load.

#### What the manual says, and why the measurement was designed this way

- **p.3:** the master volume control "Turn to control volume for **all outputs**."
- **p.36:** `MASTER` assign buttons exist **only** on Headphones / Output 1 / Output 2. The mixer's `USB 1/2`, `USB 3` and `USB 4` channels have no `MASTER` button.

The manual never reconciles these. It does **not** say the knob has no effect on channels lacking the button.

**Why it matters:** anything measuring `USB 1/2` is measuring downstream of that channel. If the knob reached it, a constant, invisible offset would ride on every capture — and a solve-then-verify round trip cannot detect it, because both measurements pass through the same attenuation.

**Mixer state — readable, but only offline.**

- **Live protocol reads: unserved.** `MixerMessage` (TMS field 5) exposes a full mixer service (`allChannelsDisplayStateRequest` → `ChannelDisplayStateChanged{ record: ChannelDisplayState{ idEnum, faderLevel, muteActive, soloActive, linkToMasterLvl, sourceActive[], … } }`, per the firmware-extracted `MixerMessage.proto`). On a properly drained line, all five mixer read requests return only a 4-byte TMS-4 connection reply and no TMS-5 stream (`probe --mixer`). `reAmpModeRequest` is itself one of those five TMS-5 requests (not a control on a different, known-working branch), so this is five untested instances of one branch, not four tests plus an independent positive control.
- **Device backup: fully readable.** The backup archive's `settingsBackup` member is plain JSON and contains `mixerSaveData` with a complete strip per channel. Measured on this unit:

  ```
  usb12 : faderLevel 1.0   muteActive false  soloActive false
          linkToMasterLvl FALSE   auxActive false  btActive false
  out1 / out2 / headphones : linkToMasterLvl TRUE
  masterVolume : 0.49999919
  ```

  `usb12.linkToMasterLvl = false` while exactly the three channels with a `MASTER` button on p.36 read `true` — the correspondence the manual leaves unstated.

- **Live protocol writes: unconfirmed.** `MixerMessage.SetMasterLevel` (TMS 5 field **16**, `float masterLevel = 1`, fixed32 wire encoding, golden-tested) was sent to the unit under several conditions — a drained line, a `Session::begin_live_edit`-warmed line, with and without `batchStatus` — and every attempt left `settingsBackup.mixerSaveData.masterVolume` and the on-screen master display bit-identical/unmoved. No confirmed write route exists today; whether the field is genuinely unserved or some other precondition is missing is open.

**Actionable now, without a live write:** because `mixerSaveData` rides the backup the app _already_ takes at startup, a real leveling **pre-flight is implementable** — warn when `usb12` is muted, soloed-out, AUX/BT-injected, or its fader is off unity. That is strictly better than a disclaimer.

**Mute, solo and AUX — measured on the unit.** Preset "Guitar" (slot 1), baseline `-22.588 LUFS` (`probe --measure-current guitar-humbucker`):

| control            | action                                             | result                    | backup confirms                                                                                                                                                                           |
| ------------------ | -------------------------------------------------- | ------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Mute**           | touch `M` on USB 1/2                               | `no signal captured`      | `usb12.muteActive: true`; restored → `-22.588 LUFS` again                                                                                                                                 |
| **Solo elsewhere** | solo `HEADPHONES` only                             | `no signal captured`      | read WHILE still engaged: `headphones.soloActive: true` at the same instant `usb12.soloActive: false` — an independent state corroboration, not just the (otherwise ambiguous) audio null |
| **Additive solo**  | also solo `usb12`                                  | `-22.588 LUFS` again      | confirms p.36 "touch additional solo buttons to add their output back into the mix"                                                                                                       |
| **AUX**            | enable AUX on USB 1/2, nothing plugged into AUX IN | `-22.588 LUFS`, unchanged | `usb12.auxActive: true`; only shows the routing toggle alone is silent — contamination with a real source connected remains untested                                                      |

**The solo finding is independently corroborated, not just inferred from silence.** `no signal captured` is a generic message that also fires when a re-amp engage is dropped, so on its own it can't distinguish "another channel is soloed" from "the tool failed to engage" — the backup read taken _while HEADPHONES was still soloed_ closes that gap by showing `headphones.soloActive: true` directly. A pre-flight built off `usb12`'s own booleans alone would miss this case entirely, since every field on `usb12` stays unchanged; it must also check "is any _other_ channel's `soloActive` true while `usb12.soloActive` is false." **BT injection** was not tested (no Bluetooth source paired) but is schema-identical to AUX (`btActive`/`btLevel` sibling fields) and expected to behave the same way.

### A2. What is the internal DSP sample rate, and is there SRC in the re-amp path? — **RESOLVED (HW-derived, fw 1.8.45)**

p.46 lists **44.1 kHz** under the heading _A/D, D/A Conversion_, and separately lists the USB audio clock as 44.1 / 48 / 88.2 / 96 kHz DAW-selectable. It states **no internal processing rate** and never relates the two. The answer below is measured, not manual-derived.

**Measured: the re-amp path IS band-limited to 44.1 kHz Nyquist (~22.05 kHz).**

The evidence is **two presets with opposite spectral tilts hitting the same edge**, which is what makes this a property of the path rather than of any preset's blocks. Both captures used flat white noise (flat to ±0.2 dB in every band to 24 kHz) re-amped through USB 1/2:

1. **A rising-HF preset** — the `gtrSplit` capture (`h_asis`, DB slot 27), **+17.0 dB at 12–16 kHz**, with no cab low-pass anywhere near the band edge — still collapses: **−12.5 dB (20–21 kHz) → −71.7 (21–22) → −98.2 (22.05–23)**.
2. **A falling-HF preset** — a chain ending in a cab sim (`ACD_CabSimTMS`, `lpf: 10500`) drops far deeper than that cab can explain. Extrapolating the cab's own skirt (three fit windows over 13–21 kHz, slopes 41–65 dB/oct) predicts −35 to −50 dB at 22–23.5 kHz; observed is **−96 to −115 dB**, an excess of **48–77 dB**.

In both cases the stopband reaches the float noise floor by ~22 kHz and holds to 24 kHz. A 48 kHz-throughout digital path would carry energy to 24 kHz; it does not.

So the p.46 44.1 kHz figure **does** govern USB-in → DSP → USB-out, and the app's hardcoded 48 kHz is a **USB-side convention only**.

Confound excluded: the captures' own headers report **48 000 Hz** (`327168 samples = 6.82 s @ 48000 Hz`), so the host was not resampling to 44.1. (Prefer that over `system_profiler SPAudioDataType`, which reports device-level rather than per-stream rates.)

_Method requirements, learned the hard way:_

- **Use flat noise, not a log chirp.** A 50 ms fade-out truncates the top of the sweep, leaving no excitation above ~23 kHz and turning the top-octave transfer into a divide-by-small artefact.
- **A single preset's rolloff proves nothing.** An un-bypassed TubeScreamer→Plexi chain reads 49 dB down by 16 kHz — that is the amp model, and it mimics a band limit convincingly. Compare presets with _opposite_ tilts, or subtract the known response of whatever blocks remain active.
- **`--reamp-wav … bypass` does NOT bypass everything.** It builds its list from `load_then_discover_blocks`, which enumerates only blocks exposing a **level-type control**; cab sims and similar stay active. Read the probe's `force-bypassed N: [...]` line and check it against the preset's actual node list before assuming a clean path.

_Related, unmeasured:_ the device carries a **global `sampleRate` setting** (`settingsBackup.sampleRate`, value `0` on this unit — see A6). Whether the ~22 kHz limit moves when that global is changed has **not** been tested, so do not assume the limit is fixed in firmware.

### A3. Preset Spillover across a preset change — **RESOLVED for tail duration and the setting's own scope; a related cross-preset residual was found but is NOT attributable to spillover (HW-derived, fw 1.8.45)**

p.17 documents a **per-preset** on/off for hearing delay and reverb tails when changing presets. The manual does not say how long tails persist, whether spillover applies to a scene change as well as a preset change, or whether it is active in re-amp mode.

**Where the flag lives, and its value across a real library:** spillover is a field in the preset's own audioGraph, `audioGraph.spillover` (boolean), readable **offline** from the device backup (`UserPresets.presetJson`). Across all **38 non-empty presets** on this unit it is `true` on **38/38** — so on this library it cannot vary row-to-row, though the field is per-preset and could in principle.

**Tail duration — measured: seconds, not milliseconds.** `probe --tail-decay` (opt-in `TMP_ALLOW_NONSCRATCH_TAIL_DECAY=1` + a `scene` parameter, 0-based `scenes[]` index) run against device slot 1 ("Guitar"), scene `Reverb`: a 0.5 s white-noise burst decayed from −46.2 dBFS at the burst's end to −91.8 dBFS by +3950 ms, an average **~11.5 dB/s** slope (ranging 9.4–15.2 dB/s across four 1 s segments — not perfectly steady) that **had not yet reached the noise floor** at the 4 s tail captured. On a real wet preset, the tail is measured in **seconds**, dwarfing the ~1.3–2.5 s inter-connection floor this repo's own capture shape imposes between a `loadPreset` and its first sample. Verified safe: the stored preset's `audioGraph.presetLevel` (the working-copy field this tool writes) read bit-identical in backups taken before and after this capture.

**Scene-change applicability — settled by the manual's own scope for the SETTING, not for audio bleed.** p.17 (physical PDF p.19) defines the setting exactly as _"On/off control (per preset) for hearing delay/reverb tails when **changing presets**"_ — preset changes only; scenes are never mentioned. This settles whether the setting's own on/off toggle is meant to apply to a scene recall (it isn't). It does **not** settle whether audio can bleed across a scene recall in practice — a scene recall (per C1/C2, a live re-scene that never calls `loadPreset`) disturbs a wet block's live state even less than a full preset reload, so there's no basis to assume it's cleaner; that remains untested.

**Cross-preset transition residual — measured across three independent precursors; NOT attributable to spillover.** A self-reload control (capturing scratch slot 400 back-to-back with itself) is invalid: it reads a suspicious exact-zero (`nonzero=0/480` per window) rather than a real noise floor, consistent with the documented device behavior that reloading the _already-current_ preset differs from loading a genuinely different one — discard that shape. Testing against three genuinely different precursors instead (a wet Guitar scene, a dry Guitar scene, and an unrelated Cello preset) resolved it cleanly:

| t (from slot 400's own burst end) | Guitar/`Reverb`-preceded | Guitar/`Arpeges`-preceded (dry) | Cello-preceded (unrelated) |
| --------------------------------- | ------------------------ | ------------------------------- | -------------------------- |
| +20 ms                            | −99.1 dBFS               | −99.7 dBFS                      | −113.1 dBFS                |
| +100 ms                           | −104.7 dBFS              | −102.8 dBFS                     | −111.6 dBFS                |
| +150 ms                           | −112.2 dBFS              | −110.4 dBFS                     | −115.2 dBFS                |
| +210 ms                           | −115.1 dBFS              | −110.9 dBFS                     | −113.4 dBFS                |

All three (verified as real, varying, nonzero captures) land in the same **~−97 to −116 dBFS** band regardless of precursor content. **This residual is a fixed property of this repo's own capture pipeline immediately after any cross-preset load, not evidence that Preset Spillover survives a reload.** It's real and worth a caveat for anything measuring near-silence right after a preset change, but the mechanism is unrelated to preceding content — what causes it (load/reconnect transient, gain-staging settle, ADC self-noise) is unmeasured. Each condition is n=1; treat "the same" as "the same order of magnitude" given the ~15-18 dB spread across conditions and timepoints. Full detail in `device-manual-gaps.md` gap 6.

The floor between `loadPreset` and the first captured sample breaks down as:

| source                     | ms          | whose constraint                                                                                  |
| -------------------------- | ----------- | ------------------------------------------------------------------------------------------------- |
| `SETTLE_AFTER_LOAD_MS`     | 400         | **ours** (`leveller.rs`)                                                                          |
| `RECONNECT_GAP_MS`         | 400         | **ours**                                                                                          |
| `SETTLE_AFTER_REAMP_MS`    | 500         | **ours**                                                                                          |
| full handshake             | few hundred | device-driven                                                                                     |
| needing a reconnect at all | —           | **device** — re-amp engages once per connection; load + engage on one connection captures silence |

Only the last row is genuinely imposed by the device. The 1300 ms is **our own settle padding**, picked for reliability in the leveller rather than measured as a minimum.

### A4. What reaches the USB 1/2 output on Split templates? — **RESOLVED: BOTH LANES REACH IT (HW-derived, fw 1.8.45)**

**Measured answer: on `gtrSplit`, both lanes reach USB 1/2.** Neither lane is dropped, and Output Assign did not gate either one away in the as-shipped state of this preset. This is the answer the manual's "either channel, or both summed depending on routing template" leaves open for a Split.

> **Measured vs inferred.** What the captures prove is that **both lanes reach the USB 1/2 capture**. That this is a _sum_ is an **inference** from the topology (the lanes never rejoin before `preset.out`, so there is no mix stage to do anything else). The captures do **not** establish the relative weighting — lane B's bypass moved the result ~6× harder than lane A's, which is fully explained by the two lanes carrying different content and blocks, but a per-lane output gain was never isolated and would look the same.

Method: flat white noise re-amped through list index 26 ("Split outputs"), capturing USB 1/2, three times — as-is, then with each lane's blocks force-bypassed. A lane that does **not** reach USB 1/2 cannot change the capture when a block inside it is bypassed, so a flat null is the "not routed" answer.

| capture                                                     | integrated LUFS | Δ vs as-is     | max \|Δ\| across bands |
| ----------------------------------------------------------- | --------------- | -------------- | ---------------------- |
| as-is                                                       | **−7.073**      | —              | —                      |
| as-is, repeated (null control)                              | **−7.071**      | **+0.002 LU**  | **0.09 dB**            |
| lane A bypassed (`G2/ACD_UserIRTMS` + `G2/ACD_TMSmallHall`) | **−5.165**      | **+1.908 LU**  | **5.58 dB**            |
| lane B bypassed (`G3/ACD_ExternalCab`)                      | **−18.714**     | **−11.641 LU** | **13.11 dB**           |

The repeat is the load-bearing control: two independent as-is captures land **0.002 LU** apart and agree within **0.09 dB in every band** — that is what a null looks like on this rig. Against that floor, lane A's departure is ~62× and lane B's ~146×. Both lanes are unambiguously in the sum.

The two lanes also leave **distinct spectral signatures**, which independently corroborates that the capture is a genuine mix of two separate paths rather than one path plus noise (Δ vs as-is, dB):

| band      | lane A bypassed | lane B bypassed |
| --------- | --------------- | --------------- |
| 20–100 Hz | −5.58           | −2.98           |
| 0.4–1 kHz | −0.88           | −0.25           |
| 1–4 kHz   | **+3.91**       | −0.49           |
| 4–8 kHz   | +2.56           | **−10.12**      |
| 8–12 kHz  | +1.92           | **−11.45**      |
| 16–20 kHz | +1.49           | **−13.11**      |

**Not established:** _why_ each bypass moves the level in the direction it does — in particular, bypassing `ACD_ExternalCab` costs 10–13 dB of HF, the opposite of the "remove a cab sim, gain brightness" intuition. That is a question about what an External Cab block's bypass does to its lane, not about routing, and it does not affect the routing conclusion.

**Still open:** whether Output Assign can gate a lane _off_ USB 1/2 (this preset was measured only in its as-shipped assign state), and whether the same holds for `micSplit`.

---

#### Firmware-derived topology — what made the measurement interpretable

p.37 says USB 1/2 carries "processed stereo output of either channel, or both channels summed depending on routing template used". For a **Split** template — two lanes that never merge — the manual does not say whether both lanes are summed onto USB 1/2, or only one, or whether Output Assign gates it.

**What the firmware settles statically.** `tm-stomp-server` embeds every template's audio graph verbatim as a JSON blob. `gtrSplit` is `numInputs: 2, numOutputs: 4`, and its `connections` give the exact lane topology:

```
preset.in 0,1 → G1 → split1
split1.out 0,1 → G2 → preset.out 0,1     (lane A — short lane, ends immediately)
split1.out 2,3 → G3 → G4 → G5 → G6 → G7 → preset.out 2,3   (lane B — long lane)
```

So the two lanes terminate on **different preset output index pairs**: lane A on 0/1, lane B on 2/3.

Verified rather than eyeballed: the blob was extracted whole (1 619 B, parses as valid JSON, 20 connections), the complete node set is `G1…G7`, `split1`, `preset`, and the only edges into `preset` are `G2.0/1 → out 0/1` and `G7.0/1 → out 2/3`. **There is no mix node in the graph** — the lanes genuinely never rejoin, which is what makes this a Split.

> Worth knowing, because it looks like a contradiction: the _preset_ carries `splitMix` with **three** mix points (`mix1/2/3`) and **three** split points (`split1/2/3`), none of which are named `mix1` in the graph above. Those are **fixed-size scaffolding arrays present on every preset**, not per-template structure — `gtrSplit` uses `split1` alone and no mix point at all. Same trap as `outputMixerSettings` below: a field existing in the preset JSON does not mean the active template uses it. Read the template's `connections`, not the preset's parameter blocks, to learn topology.

**What the firmware does NOT settle.** Which output pair feeds **USB 1/2** is not in the template blob. The obvious candidate is the preset's top-level `outputMixerSettings`, which on this unit's Split preset reads:

| source             | headphones | out1  | out2  |
| ------------------ | ---------- | ----- | ----- |
| `USB12Input`       | true       | true  | false |
| `comboOutput`      | false      | false | true  |
| `instrumentOutput` | true       | true  | false |

It is **not** the lane map. Checked across all 38 non-empty presets: **every preset carries the same three keys regardless of template**, so this is the universal output matrix — and note its destinations are headphones/out1/out2, with USB 1/2 appearing only as a _source_ (`USB12Input`, the re-amp input), never as a destination. The lane→USB-out binding lives in the server's output-device code, not in any preset or template JSON.

> **`--reamp-wav` is a write path, not a read.** `capture_full_at` sets `bypass` flags and `PresetLevel` on the target's **working copy**. Nothing is saved — a reload discards it, and slot 27's stored `presetJson` was verified SHA-256-identical before and after — but be deliberate about which preset you point it at, and keep a current backup. A guard requires `TMP_ALLOW_NONSCRATCH_REAMP=1` for any slot outside the scratch zone.

The commands used (working-copy edits only; slot 27's stored preset verified unchanged afterwards):

- A real Split preset exists — **list index 26 / DB slot 27, "Split outputs", `template: "gtrSplit"`**, found by scanning `UserPresets.presetJson` offline. Its lanes: `G1` = `ACD_DeluxeReverb65NoFx` (pre-split, common), `G2` = `ACD_UserIRTMS` + `ACD_TMSmallHall`, `G3` = `ACD_ExternalCab`. Its `splitMix` carries `splitPoints` (`levelA`/`levelB`, `enableXover`, `xoverFreq`) and `mixPoints` (`levelA`/`levelB`, `panA`/`panB`, `mainLevel`).
- Template census across the library: `gtrSeries` 25, `gtrParallel1` 11, `gtrParallel2` 1, `gtrSplit` 1 (38 non-empty presets).
- Reproduction (read-only; force-bypass is a working-copy edit that a reload discards — **never save**):

  ```
  probe --reamp-wav 26 noise.wav h_asis.wav 0.5
  probe --reamp-wav 26 noise.wav h_noG2.wav 0.5 bypass-nodes=G2/ACD_UserIRTMS,G2/ACD_TMSmallHall
  probe --reamp-wav 26 noise.wav h_noG3.wav 0.5 bypass-nodes=G3/ACD_ExternalCab
  probe --reamp-off
  ```

  Compare integrated LUFS and PSD. A lane whose isolation changes the USB 1/2 capture is in the USB 1/2 sum; a lane with **no measurable effect** is not — and that null is itself the answer.

### A5. Are Loops 3/4 in the re-amp path? — **INFERRED**

p.37 states only the exclusion: incoming USB audio is not routed through loops 1 or 2. That loops 3/4 _are_ in the re-amp path follows from their being digital and from the silence — but is never stated.

### A6. Global settings are readable offline, and the manual never says so — **HW-derived (fw 1.8.45)**

Not a manual gap so much as a capability the manual never mentions and that materially changes what a host app can check. The device backup's `settingsBackup` member is **plain JSON with 64 keys** — the entire Global Settings surface, including several that no live protocol read exposes:

| key                                         | value on this unit                       | bears on                             |
| ------------------------------------------- | ---------------------------------------- | ------------------------------------ |
| `mixerSaveData`                             | full per-channel strips + `masterVolume` | A1                                   |
| `sceneChangeBehavior`                       | `0`                                      | C2 / batched scene leveling          |
| `sampleRate`                                | `0`                                      | A2                                   |
| `instrumentInputPadActive`                  | `false`                                  | the −6 dB pad's effect on DI capture |
| `micInputGain` / `lineInputGain`            | `9` / `0`                                | mic/line-fed presets                 |
| `loop3Level` / `loop4Level`                 | `0` / `0`                                | A5                                   |
| `globalEqSaveData` + `globalEqUser1..4Data` | 10 faders + overall gain each            | global EQ on the measurement tap     |
| `tunerSaveData`                             | `{mute: true, …}`                        | the tuner-mute wording conflict (§E) |
| `outputs1`/`outputs2` + levels              | `1`/`0`, `1`/`1`                         | output routing                       |

Two consequences: **(a)** anything the app wants to pre-flight against a global setting is implementable today off the startup backup scan, with no new protocol work; **(b)** the enum encodings are **not** documented in the manual, but the firmware supplies most of the missing half.

**`sceneChangeBehavior` — encoding recovered from the firmware.** The wire type carries no names: the extracted descriptor is simply

```proto
message SceneChangeBehavior { uint32 value = 1; }
```

The names are a **C++ `Q_ENUM`**, not a protobuf enum, so they live in the Qt MOC string table of `tone-master-stomp-client`: `SceneChangeBehavior | Behavior | Retain | Revert`.

- **Settled (FW-derived):** the setting is **binary — exactly two values**, named `Retain` and `Revert` in the firmware. The manual's `MAINTAIN CHANGES` (p.35) is the UI label for `Retain`; there is no third state.
- **Settled (HW-derived): `Retain = 0`.** Confirmed by direct observation on the unit: the touchscreen (Settings → **Scene Change Behavior**) reads **`MAINTAIN CHANGES`** at the same time as the stored `settingsBackup.sceneChangeBehavior` reads **`0`**. Since `MAINTAIN CHANGES` is the UI label for `Retain` and the enum has only two keys, the ordinal is pinned: **0 = Retain, 1 = Revert.**

`sampleRate: 0` is untouched by this and remains _presumed_ 44.1 kHz (first of the p.46 list) — **INFERRED**, unconfirmed. A2 measured a 44.1 kHz stage in the re-amp path on this unit while this key read `0`, which is consistent with the presumption but does not prove the encoding.

**The `sceneChangeBehavior` write path is unconfirmed.** Field **83** on `SettingsMessage` rides the same TMS-3 branch as `reampModeActive` (30), which is provably served, and is structurally identical (`1a05 9a05 02 0801` mirrors the re-amp golden `1a05 f201 02 0801`). Writes attempted on a drained line and on a `Session::begin_live_edit`-warmed line both left `settingsBackup.sceneChangeBehavior` unchanged. TMS-3 **reads** are also unanswered (`footswitchSettingsRequest`, field 47, whose `FootswitchSettings` carries `sceneChangeBehavior` at field 12, returns only the 4-byte connection reply), so the backup is the only readback available and it shows persisted state only — a write that lands in an unflushed live copy would look identical to one that was ignored. The ordinal and the underlying behavior were instead settled directly by touchscreen observation and by **C2**, independent of this write attempt.

A `change_parameter` write against scratch slot 400, via a merely-connected (not `begin_live_edit`-warmed) session, also showed no effect — neither on the JSON readback nor on the resulting audio (`--measure-current` returned the identical LUFS before and after a knob change that should have moved it). This is consistent with the same session-liveness precondition A1's write attempts ran into: the leveller's own working writes always ride a warmed, continuously-active session, never a merely-connected one. `Session::change_parameter` must be used for any such write — a hand-rolled `send_and_dump(proto::change_parameter(..))` bypasses the wrapper's bookkeeping and silently no-ops.

**A per-block Scene Edit flag also governs whether a parameter edit is scene-exclusive.** Independent of `sceneChangeBehavior`: `SetNodeSceneEdit` (wire) / a per-block flag (`ENABLED` means a parameter change applies only to the active scene; `DISABLED` means it's shared with base). Any scene-vs-preset persistence experiment is only interpretable alongside this flag's state on the target block. The granularity is per block PER SCENE, and the "default" depends on creation order (scene-after-blocks → disabled; block-after-scenes → auto-enabled) — see **C5** for the storage shape and rules.

**e2e fixture defect found and fixed.** The four e2e scratch fixtures (list indices 400–403) carried `info.product_id: "pro"` instead of the device's own **`tmStomp`** — the unit rejects a `"pro"` preset for scene selection ("This preset was created using a newer firmware revision"), which silently blocked every scene-related engine operation on the scratch zone, not just the UI ribbon (driving `loadScene` over USB against the unfixed fixture produced bit-identical audio on every scene — no scene was ever applied). The same fixtures also shared **one** `preset_id` across all four presets, contradicting the documented invariant that preset identity is a UUID unique per preset. Both fixed in `e2e/fixtures/scenario-presets.json` and `e2e/fixtures/backup-fixture.bin` (`product_id → tmStomp`, four distinct `preset_id`s), locked by two non-regression tests in `lib.rs`'s `fixture_gates` module (one per fixture file — the drift-lock test between the two files does not itself cover these fields, since `BackupPresetRow` never carries them).

### A7. Can the TMP **emulator** stand in for the unit on these questions? — **NO for audio, UNRELIABLE for state**

Worth recording because it looks like an obvious substitute when the unit is unavailable, and it is not one. A firmware emulator exists (a separate, private project) with a drivable touchscreen UI — which makes it tempting for exactly the entries above. **Keep this section generic: tmp-companion is public, so never name that project's repo, binaries, RE tooling or VM internals here — `scripts/leak-guard.sh` refuses the commit.** Assessed against its own capabilities:

- **Audio questions (A1–A4) — categorically impossible.** The emulator's audio layer is a stimulus/silence fixture, not a DSP — no effect algorithm renders audio and there is no USB-audio device. There is nothing to measure a LUFS or a PSD _of_. No amount of effort changes this — it is not a "not built yet" problem.
- **State questions (C1, C2) — runs real UI code, but through a mocked device backend.** The client's UI logic genuinely executes, but the device side of the handshake is mocked rather than real firmware behaviour in several places. Whether a scene-change or template-change round-trip exercises firmware logic or a mock responder is **not established**.
- **Version mismatch.** The emulator is pinned to an older firmware than the unit (**1.7.75** vs **1.8.45**). Any behavioural result would need re-validating on the device anyway.

**Verdict:** do not use the emulator to close any of A1–A4 — it cannot. It _may_ be usable to form a hypothesis for C1/C2, but a result from it must never be tagged HW-derived, and given the version gap it saves little over just doing the check on the unit.

---

## B. Routing and signal path

### B1. What do `Upper Path` and `Lower Path` mean in Output Assign? — **AMBIGUOUS**

Three readings coexist in the manual, and it never disambiguates:

- **p.12** defines "an upper path and a lower path" as the two lanes of a **parallel** section (splitter → lanes → mixer).
- **p.17** describes Output Assign as routing "**instrument or microphone** signal paths" to hardware outputs — i.e. the two input channels.
- **p.18** simply lists the rows as `Upper Path` / `Lower Path` / `USB 1/2`, with no per-template variation shown.

Consequences that remain open: what the rows mean on a **Series** template, where there is no upper/lower lane at all; and whether a Split template's two lanes map onto these rows. Any code deriving channel identity from these rows is standing on an unresolved reading.

### B0. Output Assign is PER-PRESET and is applied to the global mixer on load — **RESOLVED (HW-derived, fw 1.8.45)**

The manual never says where Output Assign lives. It is drawn as a global-looking 3 × 3 grid (p.18) inside a per-preset settings area, and nothing states whether changing it affects one preset or the whole unit.

**Measured: loading a preset OVERWRITES the unit's global output-assign matrix with that preset's own `outputMixerSettings`.** Confirmed on a non-trivial pattern (not a coincidence of all-true rows): after loading slot 27 ("Split outputs"), all **9** cells of the global matrix matched that preset's block, including the three `false` ones; after loading slot 401 ("E2E Reference", all-true), the global matrix was all-true.

| preset `outputMixerSettings` source | global `mixerSaveData` key |
| ----------------------------------- | -------------------------- |
| `USB12Input`                        | `usb12Active`              |
| `instrumentOutput`                  | `instrumentActive`         |
| `comboOutput`                       | `micActive`                |

**Why this matters for leveling.** The measurement tap's routing is **not stable across a session** — it changes on every preset load, silently. Two presets measured back to back can be measured through different output matrices. This is the same class of invisible bias as A1's fader, and unlike A1 it is _certain_ rather than open. Any pre-flight built off `mixerSaveData` (see A1) must therefore be re-checked **per preset**, not once at startup, or it validates a routing state that the next load replaces.

**Corollary for A4:** that experiment's three captures all loaded the _same_ slot, and its two as-is captures bracket the lane tests at 0.002 LU apart, so the matrix was constant throughout — the result is unaffected.

**Still open:** whether editing the grid on the touchscreen writes back into the preset (making it a saved per-preset property) or only into the live mixer until the next load.

### B2. Is Output Assign's row set template-dependent? — **UNDOCUMENTED**

The p.18 screenshot shows a fixed 3 × 3 grid with all nine cells `ON`. Whether the rows change with the template is not shown or stated. The **off-state** rendering is never depicted either — only `ON` appears anywhere in the manual.

### B3. Per-category block limits — **UNDOCUMENTED in the manual**

The manual gives **no printed block counts**. p.15 documents only the behaviour: models grey out when no more can be added. The specific caps the companion enforces (`blockcaps.rs`) come from firmware inspection, not the manual — see `SKILL.md` constraints. Do not go looking for a printed number; there isn't one.

---

## C. Scenes and presets

### C1. What happens to scenes when the signal path type changes? — **RESOLVED: the switch mechanism, and per-scene BYPASS state (HW-derived, fw 1.8.45); per-scene PARAMETER overrides still untested**

p.18: changing the template "repopulates the signal path in the new template." p.20: "Changing block order or signal path type will affect all Scenes." Neither says **what happens to per-scene bypass states and per-scene parameter overrides** — preserved, remapped, or wiped.

**There IS a protocol command for changing a signal-path template:** `PresetMessage.switchTemplate{ string templateType = 1 }`, field **43**, with `templateSwitched` at field 44 as its echo. It sits alongside the node-edit surface (`--replace/--insert/--remove`), which by contrast only operates _within_ a template.

**Measured: `switchTemplate` IS served, and it is a WORKING-COPY edit that does NOT auto-save.**

| finding                          | evidence                                                                                                           |
| -------------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| the command is **accepted**      | the device echoes **`templateSwitched` (PresetMessage field 44)** — observed as `fields=2[44]`                     |
| it **changes live state**        | the live preset read `gtrSeries` → `gtrParallel1` → (switched back) `gtrSeries`                                    |
| it does **NOT auto-save**        | the stored preset (DB slot 401) was **byte-identical throughout**: still `gtrSeries`, still 2 scenes, same 4 nodes |
| blocks are **not redistributed** | under `gtrParallel1` all four blocks remained in `G1`; nothing moved to a second lane                              |

**The acceptance signal is the field-44 echo, not the read-back.** The JSON read-back is not reliable here: `currentPresetDataJson` truncates (~5 KB), and it is not established whether the push reflects the working copy or the saved preset — two reads that disagreed with the echo produced a false "no effect" before this was understood. A read that comes back `<absent>` must never be scored as "unchanged" (two failed reads compare equal and would manufacture a false negative) — the probe reports **INCONCLUSIVE** in that case.

_Method trap:_ `capture_full_preset_json` waits for the field-3 **push**, so calling `drain_until_quiet` first _eats the very thing it waits for_ — the mirror image of the read-request rule. And passing `Some(slot)` re-loads the preset, which emits **no push at all** when that preset is already current; ride the handshake's own push with `None` instead.

**Per-scene bypass state is preserved across a template switch.** Settled visually (the truncated JSON push can't answer this): a scratch preset (`gtrParallel1`, one boost pedal on the upper lane) authored fresh on the touchscreen with two scenes at opposite bypass states (Scene 1 disabled, Scene 2 enabled) was switched to `gtrSeries` over USB. On the touchscreen: the layout changed to series, the pedal survived the topology change, and recalling each scene showed the same bypass states as before the switch. Switched back to `gtrParallel1` and re-checked: unchanged throughout. **Scope, precisely:** single block, two scenes, bypass on/off only — whether a block that does NOT survive a topology change (e.g. one only present on a lane the new template removes) behaves the same way, and whether a per-scene **parameter** value (not just bypass) survives, are both untested.

**Safety conclusion:** a template switch over USB is **non-destructive** — it does not write the stored preset. The live edit does, however, **persist on the device across USB reconnects** (the unit keeps its own working copy; the USB session is only an observer), so it must be explicitly switched back or discarded by loading another preset.

**The template inventory is settled offline, and it maps 1:1 onto the manual (FW-derived).** `tm-stomp-server` embeds one graph blob per template. There are **12**, each corresponding to exactly one manual-named template:

| firmware         | manual name                    | in  | out   |
| ---------------- | ------------------------------ | --- | ----- |
| `gtrSeries`      | Instrument Series              | 2   | 2     |
| `gtrParallel1`   | Instrument Parallel 1          | 2   | 2     |
| `gtrParallel2`   | Instrument Parallel 2          | 2   | 2     |
| `gtrSplit`       | Instrument Split               | 2   | **4** |
| `gtrMicSeries`   | Instrument + Mic/Line Series   | 4   | 4     |
| `gtrMicMix`      | Instrument + Mic/Line Mix 1    | 4   | 2     |
| `gtrMicMix2`     | Instrument + Mic/Line Mix 2    | 4   | 2     |
| `gtrMicMix3`     | Instrument + Mic/Line Mix 3    | 4   | 2     |
| `gtrMicParallel` | Instrument + Mic/Line Parallel | 2   | 2     |
| `micSeries`      | Mic/Line Series                | 2   | 2     |
| `micParallel1`   | Mic/Line Parallel 1            | 2   | 2     |
| `micSplit`       | Mic/Line Split                 | 2   | **4** |

Only the two **Split** templates have 4 outputs — consistent with A4's finding that a split's lanes terminate on separate output pairs. p.18 prints all 12 templates individually, including `Mix 1` / `Mix 2` / `Mix 3` each on its own line — firmware and manual agree at 12, no reconciliation needed.

The template of any given preset is verifiable offline as `audioGraph.template`. Census over this unit's 38 non-empty presets: `gtrSeries` 25, `gtrParallel1` 11, `gtrParallel2` 1, `gtrSplit` 1 — the other 8 templates are unused here.

**Still to probe:** author two scenes on a scratch preset with distinct per-scene _parameter_ values (not just bypass), switch the template, and re-read — the open half of this question.

### C2. Are unsaved preset edits actually discarded on navigation? — **RESOLVED (HW-derived, fw 1.8.45)**

**Measured on the unit, preset slot 34 "FS LABEL TEST 2" (`product_id: tmStomp`, four FS-assigned scenes), amp block `ACD_HiwattDR103CanMod`, Scene Edit flag confirmed ENABLED:**

| step                                                 | `Master Vol` | meaning                            |
| ---------------------------------------------------- | ------------ | ---------------------------------- |
| scene `Rhythm`, as saved                             | **42 %**     | baseline                           |
| turned down, still in `Rhythm`                       | **0 %**      | unsaved edit applied               |
| `Rhythm` → `Dirty` → `Rhythm` (via footswitches)     | **0 %**      | **RETAINED across a scene change** |
| loaded another preset, returned to slot 34, `Rhythm` | **42 %**     | **DISCARDED on preset change**     |

**Two distinct answers, and the manual conflates them:**

1. **A scene change does NOT discard an unsaved edit.** The edit survived a full `Rhythm → Dirty → Rhythm` round trip.
2. **A preset change DOES discard them.** Loading another preset and returning restored the saved 42 %. So "navigating away discards your edits" is true at _preset_ granularity and false at _scene_ granularity — a distinction p.25 never draws.

**Scene Edit was ENABLED** (the per-block default, manual p.21), so the edit was scene-exclusive. A repeat with Scene Edit **DISABLED** is expected to behave differently; untested.

**This is an instance of `sceneChangeBehavior`, and the manual (p.35) says so directly:** _"Determines how Scenes are reloaded within a preset after they've been modified by **manual editing, footswitch assignments or expression pedals**."_ `MAINTAIN CHANGES` (default): _"Reloads a Scene in its last modified state."_ `DISCARD CHANGES`: _"Reloads a Scene exactly as it was last saved in the preset."_ Manual editing is named explicitly, alongside footswitch assignments and expression pedals — three trigger types, not one.

**Three of the four setting×trigger cells measured:**

| setting          | trigger                              | edit                    | result                                                                      |
| ---------------- | ------------------------------------ | ----------------------- | --------------------------------------------------------------------------- |
| MAINTAIN CHANGES | manual edit (Master Vol)             | 42% → 0%                | **retained** at 0% across recall                                            |
| DISCARD CHANGES  | manual edit (Master Vol)             | 42% → 0%                | **reverted** to 42% on recall                                               |
| DISCARD CHANGES  | FS-toggled bypass (Lightspeed pedal) | off → on via footswitch | **reverted** to off on recall                                               |
| MAINTAIN CHANGES | FS-toggled bypass                    | —                       | **not run** — untested, only inferred from the device owner's account below |

The FS-toggle arm is directly confirmed: enabling the Lightspeed pedal via its assigned footswitch while on `Rhythm`, then **re-pressing the `Rhythm` scene footswitch itself** (no need to navigate to a different scene first) reverted the pedal to disabled — treat "a same-scene FS re-press is itself a recall" as a well-evidenced working assumption for this unit/firmware rather than an independently-verified mechanism (one observation, one operator).

**The setting's real-world motivation, per the device owner, centers on the footswitch case**, which is why the manual calls it out by name alongside manual editing: before this setting existed, a footswitch-enabled block stayed enabled across a scene recall regardless of the scene's own stored state; `DISCARD CHANGES` was added so a scene recall reverts an FS-toggled block back to what the scene has stored, while `MAINTAIN CHANGES` keeps it engaged through the recall. This is owner testimony about the setting's design history, not an independent measurement of the MAINTAIN/FS-toggle cell.

**This also reconciles an earlier observation.** A `switchTemplate` working-copy edit survives USB _reconnects_, which looks like it contradicts "edits are discarded" — it does not: a USB reconnect does not reload the preset, whereas loading a different preset does. The device's working copy is owned by the device and outlives the USB session; only a preset change replaces it.

See **A6** for the e2e fixture defect (`product_id: "pro"`) that blocked scene experiments on the scratch zone before it was found and fixed.

#### What the manual says, and why it does not settle this

p.25 says only that the unit "does not automatically prompt the user to save the edited preset." It makes **no claim about the fate of the edits**. Meanwhile `Scene Change Behavior: MAINTAIN CHANGES` (the default, p.35) reloads a scene "in its last modified state", which points the other way for scene-level edits. "Navigating away discards your edits" is a common paraphrase and is **not** what the manual says.

### C3. Is Switch Link switch-level or per-function? — **UNDOCUMENTED**

On p.23's _Common Footswitch Parameters_ list, exactly three rows carry the sentence "Common to all five footswitch assignments": **Colour**, **Switch**, **Custom Label**. `Type` and `Block` lack it and are per-function. **`Switch Link` also lacks it** — so the derivation rule that establishes the other five is silent here. Default renders as `OFF`.

### C4. Rows below `Switch Link` and `Switchless Bypass` — **UNDOCUMENTED**

Both the footswitch-assignment settings list (p.23) and the EXP assignment list (p.24) render a **scrollbar thumb** implying further rows below the last visible one. Their contents appear nowhere in the manual.

### C5. Partial (bypass-only) scene overlays, the Scene Edit flag's storage shape, and `ftswStates` — **RESOLVED (HW-derived + device-owner-confirmed, fw 1.8.45)**

None of this is in the manual. The render semantics were settled with `probe --scene-node-doc` (the `--scene-doc` recipe generalized to any node) on a real user preset ("Friedman HBE", 4 scenes) imported into an empty 40+ slot and cleared afterwards; the flag rules were confirmed by the device owner from touchscreen use.

- **A `.preset` export stores the per-block-PER-SCENE Scene Edit flag structurally, not as a key.** A scene's per-node overlay is either a FULL snapshot (= flag ENABLED — the block is _isolated_ from base in that scene) or **bypass-only `{bypass}`** (= flag DISABLED — the block's knobs are _shared_ with base: a parameter change on base or in the scene affects both; only on/off is scene-tracked, since bypass is always per-scene regardless of the flag). Base-preset blocks carry no flag — it's a scene-side concept. This refines A6/p.21's "per-block flag" reading: the granularity is per block per scene.
- **How the flag defaults get set (device-owner-confirmed):** adding a scene after blocks exist → the existing blocks are NOT flagged in the new scene; adding a block after scenes exist → the block IS auto-flagged in every existing scene, snapshotting its add-time values. So a freshly added block fossilizes its **defaults** into all existing scenes (the observed wild shape: a Boost showing its add-time default `gain=5` in two scenes' overlays while base was later tuned to `2.5`), and the author disables the flag in a scene precisely to make that scene follow base.
- **Recall merges a partial overlay PER-PARAM onto base — HW-measured.** Base boost `gain=2.5`, scene A full overlay `gain=5.0`, scene B bypass-only; recalling A (renders 5.0) then B renders **2.5**, not a retained 5.0. (Consistent with `overlay_scene_onto_graph`'s per-param merge and the "absent overlay renders base" rule — this extends both to partial overlays.)
- **`scenes[].ftswStates` (one bool per `ftsw` slot) is a DERIVED CACHE, not the driver.** A crafted divergent import (`ftswStates[i]=true` while the same scene's overlay keeps that FS's block bypassed) recalls with the block **bypassed** and the rendered `ftsw.isActive` **false** — both audio and FS state derive from the block-bypass overlays; the stored `ftswStates` is ignored on recall (it is written at scene-save time). "A scene lights the upper on-off footswitches" is therefore purely the scene's bypass overlays materializing.
- **Scene-assigned footswitches (`func:"scene"`) never render `isActive=true` in the field-3 doc** even while their scene is active — the active-scene indication is `lastLoadedScene` alone.

---

## D. Songs, setlists and slot bindings

### D1. What happens to setlists when a Song is deleted? — **RESOLVED (HW-derived, fw 1.8.45)**

p.26 confirms a song can be deleted, duplicated or renamed. pp.28–29 confirm that removing a song _from a setlist_ is a separate action, and that editing a song "will affect all uses of that Song." The **cascade on outright deletion** is still never stated in the manual — the answer below is measured.

**Measured: the setlist entry is removed cleanly.** Two scratch songs added to a scratch setlist; deleting one song took the setlist from 2 songs to 1, leaving the other still correctly resolved. **No dangling entry, no orphaned reference, and the delete was not refused.**

**The firmware also rewrites slot-keyed setlist→song bindings on renumber.** A new Song is inserted at **slot 1**, not appended, so every existing song slot shifts. Inserting one song moved all 14 entries of a real 14-song setlist by exactly +1, while **every entry still resolved to the same song by name**. Removing the inserted songs restored all 14 slots exactly. So setlist→song bindings are slot-keyed **and maintained by the firmware**, not positional-and-stale.

### D2. What does reordering presets break? — **RESOLVED for Song bindings (HW-derived, fw 1.8.45)**

p.9: reordering presets "will renumber presets automatically." The manual never says what happens to Song/Setlist bindings (device-side these are slot-keyed) or to MIDI PC mappings.

**Measured: the firmware rewrites Song→preset bindings to follow the preset. Reordering is safe.**

A scratch Song was bound to list index 400 ("E2E Reference") via `assignSongPreset`, then that preset was reordered 400 → 401:

|                | song row 0 `userPresetSlot`             | resolves to                           |
| -------------- | --------------------------------------- | ------------------------------------- |
| before reorder | **401** (device 1-based = list idx 400) | "E2E Reference"                       |
| after reorder  | **402**                                 | "E2E Reference" (now at list idx 401) |

`presetSceneSlot` and the bound scene name ("Rhythm") were preserved. The binding tracked the preset rather than the slot number, so a reorder does **not** silently repoint a Song at whatever moved into the old slot. This matches D1 — consistent firmware policy.

**Method trap worth keeping:** `moveUserPreset` **appears to be a no-op if you verify on the same connection** — a re-`list_my_presets` there serves the pre-move state. Verify on a **fresh** connection. (No `presetError` is emitted either way.)

**Mechanism, found in the firmware.** The renumber is not application logic — it is a **SQLite trigger** in the device schema, visible in any backup's `databaseBackup`:

```
CREATE TRIGGER BeforeReorderPresetInUserPresetsRenumberUserPresetsSlots
    BEFORE UPDATE OF slot ON UserPresets ... WHEN NEW.slot > 0 AND OLD.slot > 0
```

It shifts the intervening rows, using **negative slot values as a temporary namespace** to dodge the `slot INTEGER NOT NULL UNIQUE` constraint mid-shuffle, then flips the signs back. The renumber is enforced at the storage layer, so it applies to _every_ path that reorders a preset, not just the `moveUserPreset` wire op that was tested. The same schema also pins the addressing rule: `CHECK (ABS(slot) >= 1 AND ABS(slot) <= 504)`, i.e. slots are **1-based**, confirming `device slot = list index + 1`.

**Note:** the SCRATCH consts in `stimulus.rs` (400–402) and `slot_write.rs` (400–402) both exclude list index 403 (`E2E Realistic`, the fourth e2e fixture) — reaching it via `--set-param`/`--switch-template`/reorder needs a code change, not an env override.

**Still open:** **MIDI PC mappings**, which address presets by bank + program number, were not tested and remain UNDOCUMENTED.

### D3. Does a backup include Songs and Setlists? — **RESOLVED: YES (HW-derived, fw 1.8.45)**

p.33 says the SD backup covers "all user presets, settings and third-party IRs". "Settings" is never enumerated, and Songs/Setlists are neither obviously presets nor obviously settings — the manual still does not say.

**Measured:** the backup archive (`probe --device-backup`, CRC-verified) is an LZ4-frame'd GNU tar with **7 members**: `productID`, `fileVersion`, `databaseBackup`, `settingsBackup`, `userIRListBackup`, `userIRFileBackup`, `acdDefaultsBackup`. `databaseBackup` is a plain **SQLite 3** database whose tables are:

`UserPresets · CloudPresets · FavoritePresets · RecentPresets · Songs · SongPresets · Setlists · SetlistSongs`

So Songs, their preset bindings, Setlists **and** setlist membership are all included — a backup is a complete restore point for them, not just for presets. `settingsBackup` (see A6) carries the global settings as JSON alongside.

_Decoding recipe (no device, no app):_ `lz4 -d backup.tar.lz4 backup.tar && tar -xf backup.tar && sqlite3 databaseBackup .tables`.

### D4. Song BPM vs global `Tap Tempo: GLOBAL` — **UNDOCUMENTED**

Both are documented as overriding stored preset tempos — Song BPM at p.27, the global tap-tempo scope at p.35. Which wins when both are active is not stated.

### D5. Song footswitch slot ordering — **INFERRED**

The p.26 performance grid and p.27 assignment grid both place INTRO / VERSE 1 / CHORUS 1 on the **bottom** row and VERSE 2 / SOLO / OUTRO on the **top** row, suggesting slots 1–3 bottom and 4–6 top. No screenshot states an index.

---

## E. Manual's own internal inconsistencies

Confirmed as genuinely printed — not extraction artefacts.

| Topic                          | Conflict                                                                                                                                                                                                                  |
| ------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **EXP pedal impedance**        | p.38 prose: **10 k–500 kΩ**. p.46 spec table: **1 kΩ–500 kΩ**.                                                                                                                                                            |
| **Master volume scope**        | p.3: controls "all outputs". p.36: `MASTER` assign exists on only three of six mixer channels. (See A1.)                                                                                                                  |
| **Block parameters per page**  | p.13 prose: "up to six … assigned to the middle six footswitches". p.13 screenshot: **seven** bound, including `AMP LEVEL` in the bottom-left slot.                                                                       |
| **Mic position order**         | p.16 prose lists "cap, cap edge, cone, cone edge". The matrix renders top→bottom as **CONE EDGE, CONE, CAP EDGE, CAP** — the prose order read bottom-up.                                                                  |
| **Add Block cancel control**   | p.15 prose: "touching CANCEL at upper left". Screenshot: the control reads **`< Back`**.                                                                                                                                  |
| **Output Assign column order** | p.17 prose lists "OUTPUT 1, OUTPUT 2 and HEADPHONES". The screen orders them **Headphones, Output 1, Output 2**.                                                                                                          |
| **USB 3/4 Pre/Post**           | p.36 prose describes one choice "out USB 3/4". The screen gives **USB 3 and USB 4 each an independent PRE/POST pair**.                                                                                                    |
| **Splitter vs mixer symbol**   | p.12 prose implies two distinguishable symbols; both the margin art and the p.18 diagram render the **same** 3-fader glyph.                                                                                               |
| **Firmware restart wording**   | p.44 prose implies restart follows completion; the device screen reads "Please restart to continue applying update."                                                                                                      |
| **Gig View scope**             | p.6 groups Cloud Presets among "the first four Preset modes"; p.7 lists Gig View as available in **My Presets, Favorites and Factory Presets** only — Cloud omitted.                                                      |
| **MIDI copy-paste slips**      | p.34 "SEND MIDI PC/CC … sets tempo for external devices that respond to MIDI clock" (clock text duplicated), and "RECEIVE MIDI CC: Select to receive MIDI **preset changes**" (should be continuous-controller messages). |
| **Tuner mute wording**         | p.32: "The tuner mutes **output** by default; tuner mutes only **input** being tuned."                                                                                                                                    |
| **Device screen typo**         | The firmware-update screen prints `www.fender/tonemaster_pro`, missing `.com`. Genuinely on the device.                                                                                                                   |

---

## F. Documentation artefacts to be aware of

### F1. The DAW Mode page has "ghost" text — **do not use its text extraction**

The PDF text layer for p.45 contains strings that are **not rendered on the page**: `DAW 99 Studio Pro`, `FOOTSWITCH ASSIGNMENT`, `VOLUME UP`, `VOLUME DOWN`, `MUTE TRACK`, `SOLO TRACK`, `ARM TRACK`, `PLAY STOP`, and an instruction bar reading "SELECT FOOTSWITCH TO **TOGGLE** …". They describe a different, track-oriented DAW layout the published manual does not show.

The **visible** layout is authoritative, and is corroborated three ways: the rendered screenshot, the page's own prose naming exactly those eight transport functions, and the tell that the real equivalent instruction bar on p.27 says "TO **ACTIVATE**". See `workflows.md` §10.

### F2. The signal-path thumbnails are schematic

The p.18 template icons draw every element as an identical square and do **not** distinguish a terminal (`INSTRUMENT` / `OUTPUT`) from a block. Square counts are not block limits and not element inventories.

### F3. The tuner screenshot is a composite

p.32 shows yellow bars lit on both sides of centre while reading `+1` cents — not a physically possible single-pitch state. Illustrative only.

### F4. Model inventory is out of scope here

The manual defers all per-model amp/cab/pedal/mic description to the separate **Model Guide**. This repo bundles it as `src/models/tmp-model-guide.json`; `tmp-companion-catalog` owns the id→name/category contract, and `product_profile.json` outranks both on on-device availability.
