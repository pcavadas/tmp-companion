---
paths:
  - "e2e/**"
  - "scripts/e2e.sh"
  - "scripts/hw-e2e.sh"
---

# e2e harness rules

Applies while editing the Playwright harness or its specs. **The stale-server false-green trap is in `CLAUDE.md`** because it fires when you _run_ the suite, not when you read these files.

## Why the harness is shaped this way

Tauri's `tauri-driver`/WebdriverIO cannot drive this app — macOS WKWebView has no WebDriver. So the dual-mode harness drives the **real React UI in headless Chromium** → an HTTP bridge → a windowless Rust backend (`tauri::test::mock_builder` MockRuntime, `bin/e2e_server.rs`) → `SimDevice` offline, or the real device online with `TMP_E2E_ONLINE=1`. One spec set under two configs. Vitest owns component-level coverage with the invoke/event bridge mocked; use the `probe` bin for real-HID paths.

## UI copy is e2e-load-bearing

The specs **regex-match user-facing strings** (`doctor.spec.ts` matches `/presets? need a look|All clear/`). Before rewording any view label or heading — especially on a design handoff — **grep `e2e/specs/` for the old phrase first**.

## Seeding and list reads

- Seed-path list reads are **TOLERANT plus a completeness floor, never `list_my_presets_strict`**. Strict decodes only terminal-frame streams and fails or garbles on back-to-back lean sessions (HW: tolerant returned 504/504 while strict returned truncated 190–236 fallbacks), and its re-arm retries themselves arm the HID open lockout.
- Online seeding runs a **FRESH `probe --seed-scenario` process BEFORE the server starts**, dodging the in-process `0xe00002c5` open lockout that aborted in-spec seeds. The seed self-repairs by sweeping stray imports — an aborted seed strands copies at the first empty slot anywhere in the bank.
- The five scenario presets live in the scratch zone at list indices 400–404 and **stay resident between runs by default** — the pristine-checking seed re-imports any drifted or stale-rev slot. Teardown unconditionally disables re-amp, sweeps strays and recalls preset 001, but clears the scenario slots **only** with `TMP_E2E_CLEAR_SCENARIO=1`, for an on-demand net-zero run. Their shapes are deliberate: `E2E Reference` (400) carries scenes AND block-acting footswitches; `Target 1/2` (401/402) are plain; `E2E Realistic` (403) is the physics-spec fixture; `E2E Hiwatt 3S` (404) backs the wipe/bake/measurement-context gates.

## `scripts/hw-e2e.sh` — the attended on-device layer

Runs the full Level + Copy happy paths against the real unit **non-destructively** (dry `--levelpreset` with no save, `--replace-held` with no commit, `--device-backup` read, `--reamp-off`). Override its `LEVEL_SLOT` / `COPY_*` env vars per unit. It is **attended, not a CI gate**, and acquires the machine-global device lock like the online `e2e.sh` path.

## Fixtures

- The offline `backup-fixture.bin` and `scenario-presets.json` **must stay in sync** — regenerate both from one script.
- **Fixture drift-lock trap:** a drift-lock or round-trip test that compares fixtures **through a typed struct** silently covers only the fields that struct carries. `info.product_id` and `info.preset_id` drifted while the lock test stayed green. Assert fixture invariants against the **RAW JSON** (`lib.rs`'s `fixture_gates`, deliberately OUTSIDE `#[cfg(feature = "e2e")]` — a gate that only compiles in a build nobody makes is not a gate). It pins `product_id == "tmStomp"` (a `"pro"` preset is rejected on the unit as "created using a newer firmware revision") and a unique `preset_id` per fixture preset.
- The preset XOR key is committed as `const PRESET_XOR_KEY: [u8;3] = *b"JLD"` in `backup.rs`. The runtime `derive_key`/`learn_key`/panic recovery was deleted — **do NOT reintroduce it**.

## Ports

`scripts/e2e.sh` derives a stable per-worktree bridge/vite port pair (offset = `cksum(worktree-path) % 200` off 7600/1421) and exports `TMP_E2E_PORT` / `TMP_E2E_VITE_PORT`. It is a `% 200` hash with no occupied-port retry, so two worktree paths can still collide — it reduces contention, it does not guarantee isolation. The one real device is serialized by a machine-global `mkdir` lock (`scripts/device-lock.sh`).
