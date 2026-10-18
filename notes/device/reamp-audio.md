# Re-amp and audio

Read this before any code that engages re-amp, captures the unit's USB audio, or opens an audio stream on any platform.
Re-amp mode replays a stimulus from the host through a preset's DSP chain and returns the processed signal over USB, with no guitar plugged in; the leveling model built on it is [Leveling](../leveling.md).

Facts are for firmware 1.8.58 unless a line says otherwise.

## Re-amp toggle

Toggle re-amp only with `SettingsMessage` (3) → `reampModeActive` (30): ON is `1a05f201020801`, OFF is `1a03f20100` [HW].
That is the route the unit's own UI and Pro Control use [RE].
Never send Mixer `setReAmpMode` (15): both routes reach the mixer, but 15 skips the Settings flag, and a later Settings ON is then acknowledged with value 1 and does nothing [HW+RE].
The Settings echo (`ReAmpModeChanged`) is sent to every host even when nothing changed, so it is not proof of engagement; a finite captured loudness is [HW+RE].
Read the real state with Mixer `reAmpModeRequest` (4) → field 9, answered to the requester on a live, heartbeated session; a silent Mixer read means the session had lapsed [HW+RE].
`Session::read_reamp_state` reads both stores (Mixer field 9 and the Settings flag through `usbSettingsRequest` (55) → field 56), and a store that never answered reads `None`, never OFF.
Setters carry no `batchStatus`: an engage with batch 4 or more is dropped with no reply, and one with batch 1 or 2 waits for a batch 3 ([Request batching](session.md#request-batching)).
Re-amp replaces Inputs 1 and 2 with the USB re-amp feed, so a unit left re-amp ON plays no guitar [RE].
A preset load never touches the re-amp state, and nothing resets it when a connection closes [RE].

## Engage and capture accuracy

Re-engaging on a held, heartbeated connection is safe: in one attended session, 10 runs with 4 re-engages and 5 ON → OFF toggles all applied, with no wedge and no USB or Mac fault [HW+RE].
Only the first capture after an engage is accurate: it reads within 0.04 LU of an isolated capture [HW].
Later captures on the same engage read 0.00–0.29 LU low, worse the longer re-amp sits engaged without stimulus, and a re-engage per capture on a held connection reads 0.06–0.09 LU low [HW].
So the code keeps one engage per capture on a fresh connection, for accuracy.
`leveller::level_preset` levels base over three fresh connections (load, measure, apply), a conservative shape kept from older firmware.
For scene and footswitch leveling only the measurement prepass reconnects, one engage per scene; the apply (per-scene `outputLevel` and the save, all plain sends with no engage) runs on one persistent session.
Every run ends with a guaranteed re-amp OFF on its own fresh connection ([Stuck re-amp](#stuck-re-amp)).

### Edits during an engage

A `setPresetLevel`, a `changeParameter` or a `loadScene` sent during an engage reaches the audio on a live session: a level step showed within 100 ms and a scene change moved the amp within ~300 ms [HW+RE].
After 1.2 s of host silence the same messages draw `connectionError` and are dropped, re-amp staying on [HW].
An engage after a `loadPreset` on the same connection was live in every trial while OUT traffic kept flowing, and failed only after 1 s of host silence following the load [HW].
The code still loads on its own connection and sets the level before engaging, a conservative shape.

### Idle gap before an engage

An engage sent after 750 ms of host OUT silence is dropped, and its capture reads the unit's stationary output floor ([Session lapse](session.md#session-lapse), [Silent captures](#silent-captures)) [HW].
Keep every last-report → engage gap at 300 ms or less.
`leveller::capture_on_session` breaks the idle of the naked shape (no force-bypass write and no reference level) with one `Session::heartbeat`: recall → 150 ms → heartbeat → 100 ms → engage (`leveller::recall_settle_ms`, `breaker_beats`, `pre_engage_settle_ms`) [HW].
That cadence measured within 0.01 LU of a longer one on every capture of the 405/410 leveling bench [HW].
Write-bearing shapes skip the heartbeat, because their writes already break the idle; `measure_scene_asis` and `arm_pair_measurement`'s zero-write base arm hand-roll the same cadence.

## Silent captures

A failed or dropped engage does not return silence: it returns the unit's stationary post-DSP output floor, a finite, plausible LUFS [HW].
Its tells are a momentary-loudness spread near 0.01 LU (a plucked stimulus through any chain spreads far wider) and a `C` that is the same across very different presets.
The floor is post-DSP, so `presetLevel` scales it and the `20·log10` model still fits: a full `--levelpreset` solve and verify landed 0.00 LU error while measuring only the floor, so solve/verify self-consistency proves nothing about the inject [HW].
The production leveller guards every measurement (`leveller::measure_floor_guarded`, `require_live`): `FLOOR_TRIP_LU` = 0.4 on the 400 ms momentary spread, a stimulus-aware disarm for near-stationary EBow captures, a quiet-gap retry, and refusal of a read that stays flat.
`leveller::is_engaged` keys on that spread, so a stationary stimulus can still read `FLOOR/SILENT`; do not retune the threshold without captures that separate a failed inject from a stationary stimulus.
`probe --stim-ab` is the diagnostic sweep.
A capture of true digital silence ("no signal captured", peak ≈ −93 dBFS) is a dropped engage too, never a wedged device; a power cycle is never the cure [HW].

### outputLevel 0

An amp's `outputLevel` multiplies its linear gain unchanged, so 0 is exact digital silence on the unit [HW+RE].
A finite LUFS is not recoverable from silence, and the measurement errors with "no signal captured".
Code that measures a deliberately muted state (`probe --mute-floor`) treats that error as a sentinel floor (`leveller::MUTE_FLOOR_SILENT_LUFS`) instead of aborting.
A `gtrSplit` amp can sit in the pre-split trunk, so it always reaches USB 1/2 and never trips the no-authority clamp.

### Presets with no blocks

A preset can carry the normal graph groups with zero populated nodes (no node carries `dspUnitParameters`), and an empty user slot reads the same way.
It re-amps as clean passthrough: a finite capture of the stimulus with only level applied, −21.2 to −26.2 LUFS through every guitar stimulus, the same for every such preset, which reads like a wedged device [HW].
Before concluding the hardware is broken, count the nodes carrying `dspUnitParameters` (`probe --slot-json <1-based slot>`).
The discriminator is spectral, and the filters must be steep: the HF (> 6 kHz) − LF (< 1 kHz) tilt with 6-pole filters read +3.53 dB dry, +3.50 dB through a no-block preset and −3.53 dB through a real chain [HW].
A 2nd-order bandpass leaks, so every band then measures overall level and a uniform delta means quieter, not unprocessed.
The shipped Doctor calibration reads that flat chain as `fizzy` and `lost` (humbucker, single-coil, acoustic) and `fizzy` alone (active) [HW].
`probe --activegraph` failing with "no preset JSON in handshake" does not mean no preset is loaded: that arm opens no dense-heartbeat session; `probe --dump-currentpresetdata <idx>` captures the live push.

## Stuck re-amp

A run-end OFF that never reads back OFF leaves the unit input-muted (`leveller::reamp_off_guaranteed`, its ladder exhausted).
Nothing on the unit resets re-amp when a connection closes, and the firmware forces it off at boot, so a USB unplug does not clear it and a restart of the unit does [RE].
Neither half is checked on a unit ([open question](../open-questions.md#re-amp-across-a-usb-unplug)).
The user's recovery is "Restart your Tone Master Pro" (`leveller::REAMP_STILL_ON_WARNING` and the Summary); the developer's is `cargo run --bin probe -- --reamp-off`, which sends the Settings OFF without a restart.
Every leveling result carries the typed `reamp_still_on` (a failed command's typed error `reampStillOn`) beside the `reamp_warning` text, and the frontend reads the flag, never the text; a batched command returns it beside its rows, so a batch with no rows still carries it.
Cancel never returns early past an engage, and the post-cancel restore and deferred-save cleanup always run to completion.
The shipped backstop is `leveller::ReampRunGuard`, held by `device_gate::with_released_seize_blocking` around every device command: a command that engaged and then panicked, returned early or skipped its own OFF still gets the OFF ladder on a fresh connection.

## Sample rate

The unit's DSP and I²S run at a fixed 44.1 kHz; the global `sampleRate` setting is only stored and echoed [HW+RE].
The USB side converts the host rate (44.1, 48, 88.2 or 96 kHz) to and from 44.1 kHz with a fixed-ratio synchronous SRC, and the re-amp round trip is pitch-exact and slip-free at all four rates [HW+RE].
So capture content above ~22.05 kHz is anti-alias skirt, not preset tone: flat-noise captures collapse to the float floor above it on presets of opposite spectral tilt [HW].
48 kHz is a host and app convention: the stimulus WAVs are 48 kHz and `audio.rs`'s `pick_config` opens both streams at the stimulus rate, so the host must offer 48 kHz.
`probe_stimulus_path` resolves `resources/samples/<id>.wav` relative to the current directory, so a probe binary run from outside `src-tauri` cannot find its stimulus.

## Output routing

The re-amp inject goes out on USB channel 3 (`REAMP_INSTRUMENT_OUT_CH`, 0-based 2), and the processed signal returns on USB 1/2.
A preset load overwrites the unit's output-assign matrix from that preset's `outputMixerSettings` (`USB12Input` → `usb12Active`, `instrumentOutput` → `instrumentActive`, `comboOutput` → `micActive`; all 9 cells checked) [HW].
So the measurement tap's routing is not stable across a batch, and a pre-flight that checks it must re-check per preset.
The unit's master volume knob is not in the USB 1/2 path: 0.001 LU across 50 % → 80 %, and −23.8 against −23.6 LUFS at zero and up [HW].
The USB 1/2 channel fader is in the path: −6.48 LU at its −10 dB mark (HW, before fw 1.8.58).
So quiet leveling is the master knob down on the unit; on Windows the app also handles the endpoint volume ([Platform audio](#platform-audio)).

## Platform audio

### Linux (ALSA)

The unit's audio interface is USB id `1ed8:0047`, not HID's `0044`: `/proc/asound/cardN/usbid` shows `0044` as a MIDI-only card and `0047` as the 4-in/4-out audio card [HW].
`audio.rs`'s Linux `find_device` walks `/proc/asound` for the Fender-vendor (`0x1ed8`) card that has a `pcm*` entry.
`hw:CARD=<id>,DEV=0` offers `S32_LE` only, 4 channels, 44.1–96 kHz, and no F32 [HW].
`pick_config` accepts I32 and converts at the stream-callback boundary with `dasp_sample`'s exact 2^31 scaling (`f32_to_i32`, `i32_to_f32`); any other scale factor silently mis-calibrates every LUFS reading.
`plughw:` is not a safe substitute: it advertises 1–64 channels, so the smallest fit of at least 3 would go through `plug`'s own remix instead of the 1:1 mapping `REAMP_INSTRUMENT_OUT_CH` needs (chosen for that risk, not measured broken).
Resolve the device by ALSA pcm_id, never by name: cpal lists a non-functional `usbstream:CARD=Pro` hint named "Tone Master Pro" ahead of the real "Tone Master Pro, USB Audio" PCM, so `find_device` matches the exact pcm_id (`description().driver()`, for example `hw:CARD=Pro_1,DEV=0`) [HW].
`audio::enumerate()` keys its map on `(name, driver)`, because several ALSA PCMs share one display name.
PipeWire/WirePlumber holds the PCM persistently, so `hw:`'s exclusive open fails `EBUSY` [HW].
The fix is a WirePlumber rule matching `device.vendor.id = "0x1ed8"` and `device.product.id = "0x0047"` with `device.disabled = true` ([CONTRIBUTING.md](../../CONTRIBUTING.md#developing-on-linux)); `audio.rs`'s `stream_error` appends it to the busy error.
`hw:` negotiates the rate with the hardware directly, and the unit's `hw:` interface offers 48 kHz natively [HW].
`probe --measure-current`, `--levelpreset` and `--doctor` run on Linux with the re-amp rules unchanged [HW].

### Windows (WASAPI)

Shared mode offers only the endpoint's Windows default format, and cpal has no exclusive-mode path [HW].
With Fender's Thesycon driver the unit enumerates as `Speakers (Fender Tone Master Pro)` with 2 channels and `Line (Fender Tone Master Pro)` with 4 channels at 44.1 kHz only (`probe --audio-devices`), so re-amp fails until the user sets Quadraphonic + 48000 Hz on the output and a 4-channel 48000 Hz default on the input.
`audio::host_config_hint` names that fix in the error, and `CONTRIBUTING.md` carries the click path.
The class-driver `Tone Master Pro` (`PID_0044`) endpoint disappears once the Thesycon driver binds `PID_0047`.
Shared mode applies the Windows endpoint volume to the inject and the capture: at an 8 % slider a clean scene measured −34.6 LUFS through the inject against −14.3 played live [HW].
`audio::endpoint_volume` holds the unit's own endpoints (matched by name, never the user's speakers) at 100 % and unmuted for each re-amp session, restores the previous values on drop, and warns when it changed something.
Pinning 100 % permanently is wrong: the same slider can keep a Windows monitoring loop quiet, and a pinned version audibly doubled the user's live guitar.

## Capture window

A leveling capture integrates a 6 s stimulus plus a 0.8 s tail, and that window is load-bearing [HW].
Presets are not stationary under gated integrated LUFS: delay and reverb presets build up at the start and decay in the tail, so an early exit, a dropped tail or a skipped preroll each shift the reading preset-dependently (clean +0.30, delay +0.16, reverb −0.02 LU) [HW].
The adaptive early-exit capture is opt-in (`audio::MeasureOpts::adaptive`); leveling uses `MeasureOpts::full`, and how to validate a change against the full-capture oracle is in [Leveling](../leveling.md).
HW LUFS varies ~0.12 LU run to run on these presets, so a bias needs a many-sample A/B, never 2–3 captures [HW].
`audio::LiveReamp` keeps a bounded ring (`LIVE_RING_SECS`): an unbounded capture buffer once exhausted the Mac's memory, so never accumulate captures without a bound, and reuse one stream pair per preset rather than one per scene.
