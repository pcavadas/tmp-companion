# TMP Companion

A macOS desktop app that interoperates with a Fender Tone Master Pro **you own**,
over USB, to read, level, copy, and back up **your own** presets. It is an
independent, unaffiliated project — see [NOTICE](NOTICE) and [INTEROP.md](INTEROP.md).

Built with [Tauri 2](https://tauri.app) (Rust backend + React/TypeScript frontend).
macOS-only on Apple Silicon (it uses CoreAudio for re-amp measurement and IOKit
for exclusive USB-HID access).

## Features

Click-only five-tab UI:

- **Level** — even out preset loudness to a LUFS target by re-amping a synthetic
  stimulus through each preset's DSP chain and measuring the processed output, then
  writing the solved level back (opt-in). Per-instrument profiles and per-scene
  leveling included.
- **Copy** — copy signal-chain blocks between presets.
- **Songs** — device-backed song / setlist browsing and editing.
- **Catalog** — a reference of amp / cab / effect models with original artwork and
  per-block DSP cost.
- **Settings** — loudness targets, instrument profiles, calibration, playback-level
  compensation.

Everything acts on your own device and your own presets. It does not publish,
upload, or share presets.

## Download

Grab the latest `.dmg` from the [Releases](https://github.com/pcavadas/tmp-companion/releases)
page. macOS 12+ on **Apple Silicon** only.

The app is code-signed and notarized, so it opens normally — just copy
**TMP Companion.app** to `/Applications` and launch it.

Plug in your Tone Master Pro and close Fender's Pro Control first — the app needs
exclusive USB access.

## Build & run

Requires [Bun](https://bun.sh) ≥ 1.3 and a Rust toolchain (stable).

```bash
bun install              # install frontend deps
bun run tauri dev        # launch the app

bun run build            # production frontend bundle
bun run tauri build      # package the macOS app
```

Plug in your Tone Master Pro and close Fender's Pro Control first — the app needs
exclusive access to the unit.

### Tests

```bash
bun run test                          # frontend (Vitest)
bunx tsc --noEmit                     # frontend typecheck
bun run lint                          # strict eslint (--max-warnings 0)
cd src-tauri && cargo test --lib      # Rust unit tests
```

> Run `bun run build` before the Rust checks in a fresh clone — Tauri's build step
> needs `dist/` to exist.

## For contributors & agents

Working in this repo (human or AI agent)? Start with [`CONTRIBUTING.md`](CONTRIBUTING.md), then:

- [`CLAUDE.md`](CLAUDE.md) — the architecture map + the running log of load-bearing invariants and gotchas.
- [`notes/`](notes/) — topic deep-dives (protocol, leveling, write-safety, block-copy, songs).
- [`INTEROP.md`](INTEROP.md) — the interoperability / legal posture.

Behaviour is firmware-version dependent; validated on Tone Master Pro firmware **1.7.75** and **1.8.45**.

## License

[MIT](LICENSE). Independent and unaffiliated with Fender; third-party names are
used nominatively — see [NOTICE](NOTICE).
