# Physical I/O, USB routing & setup recipes

Source: Owner's Manual v1.8, printed pp. 2, 4, 33, 37–41, 46. Page refs are **printed** pages.

Companion to `SKILL.md` (product data model). This file answers the _rigging_ questions: which jack goes where, what level to set, and how to get the unit into a given live or studio configuration.

---

## 1. Jacks, exactly as silkscreened (p.4)

Read off the rear-panel artwork, so these are the literal panel strings.

**Top row, left → right:** `MIC/LINE` · `LOOP 3` · `LOOP 4` · `OUTPUT 1` (XLR pair, lettered `LEFT` / `RIGHT`) · `AUX IN` · `PHONES` · `EXP 1` · `EXP 2` · `micro SD` · `USB`

**Bottom row, left → right:** `INSTRUMENT` · `LOOP 1` · `LOOP 2` · `OUTPUT 1` (¼" pair, `LEFT` / `RIGHT`) · `OUTPUT 2` (`LEFT` / `RIGHT`) · `TOE SWITCH` · `AMP CTRL` · `MIDI` (`IN` and `OUT/THRU`)

Each loop's two jacks are individually lettered `SEND` and `RETURN`. Ground lift is silkscreened `GND` / ⏚ / `LIFT`, sited between the OUTPUT 1 LEFT and RIGHT columns. Mains plate reads `100-240V ∼ 50/60Hz`, `40W`.

> **Stereo loop channel assignment is panel-printed:** between the `LOOP 3` and `LOOP 4` boxes the panel reads `LEFT ·········· STEREO ·········· RIGHT`. So when configured as one stereo loop, **Loop 3 = LEFT, Loop 4 = RIGHT.** This appears nowhere in the prose.

### The firmware-update button (p.44)

A small recessed round button **immediately left of the ¼" `PHONES` jack** and right of the 3.5 mm `AUX IN` jack, with an LED directly above it. Location is image-only — the prose never says where it is.

---

## 2. Output levels — the setting that governs every recipe

Both outputs carry an independent **instrument / line** switch and an independent **stereo / mono** switch (Global Settings → I/O → Outputs, p.33).

| Destination                                                                 | Level              | Why                                                                         |
| --------------------------------------------------------------------------- | ------------------ | --------------------------------------------------------------------------- |
| FR cabinet (Fender FR-10 / FR-12), PA, studio monitors, recording interface | **LINE** (default) | manual states it explicitly for "optimal signal-to-noise ratio" (pp. 40–41) |
| Instrument amplifier (front end, or its FX return)                          | **INSTRUMENT**     | pp. 40–41                                                                   |

Effects loops 3/4 have their own instrument/line switch (default **INSTRUMENT**, "typical for stompbox pedals"; choose **LINE** for rack effects).

**Headroom is not symmetric between the two outputs** (p.46) — relevant whenever you split a rig across both:

|                       | Max output, line | Max output, instrument |
| --------------------- | ---------------- | ---------------------- |
| `OUTPUT 1` balanced   | 23.8 dBu         | 14.1 dBu               |
| `OUTPUT 1` unbalanced | 18.0 dBu         | 11.2 dBu               |
| `OUTPUT 2`            | 15.8 dBu         | 10.9 dBu               |

`OUTPUT 1` also has the ground-lift and the XLR pair; `OUTPUT 2` is ¼" only.

---

## 3. The four documented setups (pp. 40–41)

The diagrams draw the unit from the **top**, so no diagram physically locates a jack, and **none annotates a cable type** — the only cable-type guidance is prose. Labels are generic pills (`OUTPUT`, `INSTRUMENT`, `FX LOOP: SEND` …), never numbered, so "which OUTPUT jack" and "which loop" are the user's choice.

### 3.1 Instrument or Mic/Line → FR cabinet (p.40)

Three cables: guitar → `INSTRUMENT`; microphone → `MIC/LINE`; `OUTPUT` → one FR cabinet.
Set rear-panel outputs to **LINE**.

> The diagram draws exactly one output cable and one cabinet. It does not state mono vs stereo — _inference_: this is the mono/one-cab illustration.

### 3.2 Effects pedalboard with an external amplifier (p.40)

Two cables: guitar → `INSTRUMENT`; `OUTPUT` → amp input. **Straight into the front of the amp — nothing touches the amp's effects loop.**

- Use **effects-only presets, with no amp and no cabinet blocks** (you are using the real amp's preamp).
- Set rear-panel outputs to **INSTRUMENT**.

### 3.3 Four-cable method (4CM) with an external amp (p.41)

Puts TMP effects **both before and after** the external amp's preamp. Requires an amp with a series effects loop.

**The four cables, in signal order:**

| #   | From                | To                    |
| --- | ------------------- | --------------------- |
| 1   | Guitar output       | TMP `INSTRUMENT`      |
| 2   | TMP `FX LOOP: SEND` | Amp `INPUT`           |
| 3   | Amp `FX LOOP SEND`  | TMP `FX LOOP: RETURN` |
| 4   | TMP `OUTPUT`        | Amp `FX LOOP RETURN`  |

Resulting topology: blocks placed **before** the FX-loop block in the signal path hit the amp's preamp (drives, wah, compressor); blocks placed **after** it sit in the amp's loop (delay, reverb, modulation).

**Required settings — all three, or it will not work as intended:**

1. Use **effects-only presets** — no amp block, no cabinet block.
2. Set rear-panel outputs **and** the effects loop to **INSTRUMENT** level.
3. **Add the FX Loop block to the signal path.** The rear-panel jacks are not in circuit until the corresponding loop block is placed — this is the step most often missed.

> Use loops **3/4** for 4CM: loops 1/2 sit before the A/D at a fixed position and cannot be moved, so they cannot be placed mid-chain. The diagram does not say which loop to use; this follows from the loop-placement rules in `SKILL.md`.

### 3.4 Studio monitoring / PA (p.41)

Guitar → `INSTRUMENT`; microphone → `MIC/LINE`; and **two cables from `OUTPUT` to a monitor pair** — the only diagram in the manual drawing more than one output cable.
Set outputs to **LINE**.

> _Inference:_ two cables into a monitor pair is a stereo L/R connection. The diagram asserts neither stereo nor jack identity.

### 3.5 Stage power amp **and** front-of-house PA simultaneously

**The manual documents no such recipe** — this is assembled from parts, and the assembly is inference, not doctrine.

The mechanism exists and is well supported: `OUTPUT 1` and `OUTPUT 2` are independent, each with its own level and stereo/mono switch (p.33), and per-preset **Output Assign** (p.18) is a 3×3 matrix routing `Upper Path` / `Lower Path` / `USB 1/2` to `Headphones` / `Output 1` / `Output 2` independently.

A workable configuration:

- **`OUTPUT 1` → FOH**, set to **LINE**, XLR, ground-lift as needed. Full amp + cab modelling.
- **`OUTPUT 2` → stage power amp**, set to **INSTRUMENT** (or line, depending on the power amp's input).
- If the power amp drives a **real guitar cab**, that path should use the **External Cabinet** option with an appropriate **Speaker Impedance Curve** rather than a modelled cab + IR (p.16) — otherwise you get cab-on-cab.
- To send _different_ processing to each destination, use a **Split** template (`Instrument Split`) and assign `Upper Path` → Output 1, `Lower Path` → Output 2 in Output Assign.

Two caveats worth stating: `OUTPUT 2` has ~8 dB less headroom than `OUTPUT 1` (§2), and see `open-questions.md` — the manual never disambiguates what `Upper Path` / `Lower Path` mean on a non-parallel template.

---

## 4. USB audio (p.37)

TMP is a **4-in / 4-out** USB 2.0 interface. Two modes, in Global Settings → I/O → USB. **Reamp mode resets to OFF at power-off.**

|                 | Standard (default)                                                        | Reamp                                               |
| --------------- | ------------------------------------------------------------------------- | --------------------------------------------------- |
| USB **out** 1/2 | processed stereo — one channel, or **both summed**, depending on template | same                                                |
| USB **out** 3   | instrument channel, **dry**                                               | disabled                                            |
| USB **out** 4   | mic/line channel, **dry**                                                 | disabled                                            |
| USB **in** 1/2  | monitor mix from computer; routable to physical outs via Output Assign    | same                                                |
| USB **in** 3    | disabled                                                                  | **→ instrument channel**, mutes the instrument jack |
| USB **in** 4    | disabled                                                                  | **→ mic/line channel**, mutes the mic/line jack     |

Re-amp audio enters **at the first processing block** and **does not pass through analog loops 1/2** (they are pre-A/D). Both physical input jacks are muted while re-amp is on.

> **USB out 3 (dry instrument) has no limiter and clips at 0 dBFS** for hot pickups played hard (live-confirmed) — never roll the guitar volume back to avoid it, that just under-drives the amp and darkens the tone (see `SKILL.md`). **The re-amp inject is NOT AGC'd** — its amplitude directly drives the block's nonlinearity, so a hotter injected signal genuinely drives the chain harder; do not apply gain processing to the injected track expecting the device to compensate.

**Clocks are separate specs (p.46):** internal A/D–D/A conversion is **44.1 kHz / 32-bit**; the USB audio clock is **44.1 / 48 / 88.2 / 96 kHz, DAW-selectable**. The manual states no internal DSP rate and does not relate the two — but measurement does: a **44.1 kHz stage is in the re-amp path** (`open-questions.md` A2, HW fw 1.8.45), so nothing above ~22 kHz in a USB capture is preset tone. The host side still runs at 48 kHz.

---

## 5. Effects loops — the asymmetry that drives rigging

|                         | Loops 1 & 2                                                | Loops 3 & 4                                                      |
| ----------------------- | ---------------------------------------------------------- | ---------------------------------------------------------------- |
| Type                    | Mono, **analog relay**                                     | Digital send/return                                              |
| Position                | **Fixed**, right after the instrument jack, **before A/D** | **Movable**, anywhere after Loop 2                               |
| Path                    | Instrument only                                            | Instrument **or** Mic/Line                                       |
| Config                  | Two mono                                                   | 2× mono, or **one stereo (Loop 3 = L, Loop 4 = R)** — per preset |
| Level                   | —                                                          | Global instrument/line switch                                    |
| In re-amp path?         | **No**                                                     | Yes                                                              |
| On/off saved per preset | Yes                                                        | Yes                                                              |

Loops 1/2 being pre-conversion is why they suit fuzzes and vintage wahs that want to see pickup impedance directly — and why they are invisible to USB re-amping.

Cabling any loop: TMP loop `SEND` → external unit input; external unit output → TMP loop `RETURN`.

---

## 6. Control I/O

- **EXP 1 / EXP 2** — ¼" TRS. Signal on **tip**, power on **ring**. Polarity reversible globally. Compatible with the Fender Tread-Light. _Impedance is stated inconsistently by the manual: 10 k–500 kΩ in the p.38 prose, 1 kΩ–500 kΩ in the p.46 spec table._
- **Toe Switch** — ¼" TS. Latching (default) or momentary.
- **Amp Ctrl** — ¼" TRS carrying **two independent latching contact closures**: **tip = Amp Control 1, ring = Amp Control 2**. Needs a **TRS-to-dual-TS insert cable** to break out both (the manual's only illustration of this is an unlabelled Y-cable on p.38). Polarity reversible. Also drives "bypass for products that use tip/sleeve shorting jacks", not just channel/reverb switching.
- **Aux In** — ⅛" TRS stereo. Level is controlled **only by the source device**; routable to any output from the mixer.
- **Headphones** — ¼" stereo. Follows master volume by default; the mixer also has an independent headphone fader.
- **micro SD** — any **micro** SD / SDHC / SDXC, FAT32 or exFAT. Not included.

---

## 7. Selected specifications (p.46)

|                     |                                                                                                    |
| ------------------- | -------------------------------------------------------------------------------------------------- |
| Type / power        | PR 5642 · 40 W · 100–240 V, 50/60 Hz                                                               |
| Dimensions / weight | 371 × 261.4 × 96.4 mm (14.6 × 10.3 × 3.8") · 4.0 kg (8.8 lb)                                       |
| Instrument in       | 1 MΩ / 330 kΩ / 22 kΩ selectable, unbalanced. Max **11.2 dBu**, or **17.2 dBu with the −6 dB pad** |
| Mic in (XLR)        | 1.8 kΩ balanced, max 15.7 dBu, +48 V switchable                                                    |
| Line in (¼")        | 1 MΩ. Max 20.1 dBu balanced / 15.3 dBu unbalanced                                                  |
| FX loop 3/4 returns | 10 kΩ. Max 15.8 dBu line / 10.9 dBu instrument                                                     |
| FX loop 3/4 sends   | 170 Ω. Max 15.8 dBu line / 10.9 dBu instrument                                                     |
| Aux in              | 10 kΩ unbalanced, max 9.8 dBu                                                                      |
| Output 1 impedance  | 200 Ω line; 290 Ω balanced / 400 Ω unbalanced at instrument level                                  |
| Output 2 impedance  | 170 Ω                                                                                              |
| Headphones          | ≥16 Ω, 110 mW × 2                                                                                  |
| Conversion          | 32-bit, **44.1 kHz**, 117 dB ADC / 112 dB DAC, 20 Hz–20 kHz +0.1/−0.7 dB                           |
| USB clock           | 44.1 / 48 / 88.2 / 96 kHz, DAW-selectable                                                          |

Note the pad is exactly compensating: it attenuates 6 dB and raises max input by 6 dB.

---

## 8. Quick start (p.2)

1. Guitar/bass → `INSTRUMENT` with a ¼" instrument cable.
2. XLR from **`OUTPUT 1` left** → FR cabinet or studio monitors; **or** headphones → ¼" `PHONES`.
3. Power on the TMP.
4. Power on the cabinet/monitors.
5. Raise volume slowly. Suggested master ≈ **50 %**.

**Phantom power must be disabled when connecting an XLR cable to a mixer or other interface** (p.38).
