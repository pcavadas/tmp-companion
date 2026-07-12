# Leveling & Doctor performance — timing budget, adopted speedups, dead ends

HW-measured on fw 1.8.45 (2026-07-12/13, `probe` bin, slots 5/11/13/14/19). This doc
exists so nobody re-attempts a refuted optimization or re-measures a settled budget.

## Where the time goes (per item, before the 2026-07 speedups)

| Path                           | Wall clock | Captures | Fixed waits around each capture                                                                                                              |
| ------------------------------ | ---------- | -------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| Preset Base (probe, verify on) | 26.7 s     | 2        | per capture cycle ≈ 11 s: 6.8 s window + 2×0.66 s handshakes + 1.2 s post-load settle + 0.5 s post-engage + 0.8 s re-amp on/off + 0.8 s gaps |
| FS scene (full job)            | ≈ 22 s     | 2 min.   | as-is measure + unconditional verify + save-on-fresh-conn                                                                                    |
| Footswitch (dry, 2 iters)      | 30.1 s     | 3+       | reload + 2 seeds + secant + live-edit write (1.6 s warmup)                                                                                   |
| Doctor sound (probe)           | 19.2 s     | 1        | 8.5 s window (2.5 s Doctor tail)                                                                                                             |
| Doctor prescription apply      | ≈ 26 s     | 2        | fresh before + after A/B clips                                                                                                               |

The capture window itself (6 s stimulus + 0.8 s / 2.5 s tail) is 40–60 % of every path
and is LOAD-BEARING (see the capture-window gotcha in CLAUDE.md) — everything adopted
below trims the waits _around_ it.

## Adopted (the 2026-07 speedup set — all HW-validated)

1. **`SETTLE_AFTER_LOAD_MS` 1200 → 400.** Byte-identical measured C / presetLevel /
   verify error on a dry (slot 11) and a wet delay (slot 5) preset; verify captures
   confirm writes also land at 400. `TMP_SETTLE_AFTER_LOAD_MS` overrides for bisects.
   `SETTLE_BEFORE_WRITE_MS` stays 600 — on the no-verify write branch a dropped write
   is maximally silent; 200 ms isn't worth that risk class.
2. **`Session::connect_lean()`** — handshake pump windows ×0.25 (660 → 165 ms), used
   ONLY by pure measure/capture connections: never by sessions that read handshake- or
   push-accumulated data (`list_my_presets`, `active_preset_name`, `confirm_active`,
   slot reads) or write-then-save (the scene-write cliff + chunked ftsw edits were NOT
   in the A/B). The request byte-sequence is identical (unit-tested); ×0.25 and ×0.1
   both measured byte-identical across ~10 sessions. `TMP_HANDSHAKE_SCALE` scales
   either window set for diagnosis.
3. **Doctor consecutive-scene load skip** (`doctor_skip_load`): a scene sound skips
   the per-sound load connection when the previous sound was the same preset, made no
   force-bypass writes, and succeeded. Basis: the working copy survives reconnects
   (HW-proven, see below) + the capture connection recalls the scene either way; a
   no-reload scene capture measured 0.001 LU from a fresh reload (and re-proven after
   a device power cycle). Base/footswitch sounds always reload.
4. **Doctor before-clip cache** (`BEFORE_CACHE`, single entry): `doctor_apply` reuses
   the stored preset's own-level BEFORE clip across consecutive applies on the same
   sound (key = list index + name + stimulus path + calibration). A cache hit still
   RELOADS the slot (the load both feeds `confirm_active` and discards a stale dirty
   edit buffer — never skip it). Cleared by `doctor_save`, the leveling/copy save
   commands, and device detach. Saves ~11 s per 2nd+ apply on a sound.
5. **FS isolation-once**: `measure_footswitch` sends the forced engaged-bypass list
   only on the first successful capture; the working copy persists across reconnects.
   `TMP_FS_ISOLATION_EVERY` restores the per-capture re-send.

Measured effect: preset 26.7 → 24.2 s, Doctor sound 19.2 → 17.4 s, FS job 30.1 →
25.8 s; Doctor multi-scene presets and repeat prescription-applies save several
seconds more per item (skip-reload / cache).

## Working-copy persistence (the fact behind 3–5)

Working-copy edits survive HID disconnect/reconnect: after a session forced
`bypass=false` and dropped, a fresh zero-write connection measured the forced state
exactly (−25.285 vs base −23.78, `probe --measure-forced` + `--measure-current`).
Only a `loadPreset` (or save) resets it.

## Refuted / dead ends — do NOT re-attempt as drop-ins

- **`saveCurrentPreset` chained before `loadPreset` on ONE connection silently DROPS
  the save** (the load lands; stored bytes unchanged; no `presetError`).
  `probe --save-load-test`. The save-then-fresh-connection choreography is load-bearing.
- **Audio stream reuse across captures**: measured stream cost is only ~211 ms/capture
  (resolve 69 + build/play 90 + teardown 52, `TMP_AUDIO_TIMING=1`) — not worth touching
  the BatchedLive-adjacent code (see the retired closed-loop harness in
  `notes/leveling.md`).
- **Adaptive early-exit capture**: −3.8 s/capture but +0.17 LU on slot 11
  (`probe --measure-adaptive`) — a re-baseline, product decision, not a drop-in
  (see the capture-window gotcha).
- **Capture-window / tail trims**: re-baseline (documented in CLAUDE.md).
- **Doctor tail is graph-aware?** Untried: 0.8 s tail when the chain has no time-based
  blocks would save 1.7 s/sound but changes `washed`'s calibrated measurement space.

## Remaining levers (not taken)

- **Drop the Base verify capture** (product call): every healthy verify reads
  0.00 err (the C-model is exact) and the floor guard covers inject failure
  independently — would take a no-FS preset from ~24 s to ~12 s. Verify is
  insurance; removing it is a policy decision, not an engineering fix.
- Scene verify capture must STAY: it is also the correction-pass trigger and the
  leak-to-base detector.

## Reliability observations (pre-existing, surfaced by this work)

- **Single-capture outliers, ~1-in-7 rate on repeated Doctor sweeps**: one run
  read a 3 dB band shift (flipping muddy→boomy), another a −80 dB tail sentinel
  (empty tail window); the flanking runs were byte-identical. A single Doctor
  capture can misread; repeated runs are the arbiter (the noise-floor rule).
- **Numbered-scene recall over USB is boot-state-dependent on some presets**: slot
  13 scene 0 measured −13.77 consistently one day and −21.1 consistently the next
  (same stored bytes, every timing config) — the scene-0 anomaly (`notes/leveling.md`)
  extends across power cycles; slots 0/1's numbered scenes read the −67 output floor
  outright. Scene-leveling absolute results on such presets deserve by-ear review.

## Re-validation oracles (run after any timing change)

```bash
cd src-tauri && export TMP_LEVELLER_STIMULUS=$PWD/resources/samples/guitar-humbucker.wav
cargo run --bin probe -- --levelpreset 11 -30    # C=-18.27, err ≤0.05 LU
cargo run --bin probe -- --levelpreset 5 -30     # err ≤0.12 LU (wet class)
cargo run --bin probe -- --doctor 11 guitar-humbucker  # muddy+lost, balance +4.6 +1.9 -14.4 +0.4 +10.0 -2.5
cargo run --bin probe -- --level-footswitch 14 6 G1 ACD_BluesBreaker gain -30  # engaged -22.57
```

(Absolute scene LUFS values are boot-state-dependent — compare within a session, not
across power cycles.)
