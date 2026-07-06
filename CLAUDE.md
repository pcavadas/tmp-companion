# tmp-companion

Tauri 2 (Rust backend + React/TypeScript frontend) **macOS-only** desktop app that auto-levels real Fender Tone Master Pro presets to a LUFS loudness target by driving the device over USB in **re-amp mode** — no guitar plugged in. It plays a synthetic guitar-like sample through a preset's full DSP chain, captures the processed USB-Out, measures loudness, computes the `presetLevel` that hits the target, and (opt-in) saves it back to the preset. It renders its **own** click-only 5-tab UI — Level · Copy · Songs · Catalog · Settings (`src/views/`, route keys = the labels). The audience is thousands of users, many not comfortable with computers: fewest possible clicks per action; every feature ships whole (no subsetting).

## Before you code (every task)

1. **Load the guidance for the area you're touching** — the routing table below. Don't work from memory of a similar codebase.
2. **Grep before trusting any doc symbol.** The source tree is authoritative; docs drift. If a doc names a file/function that doesn't exist, flag the drift and follow the code.
3. **Device-facing change → a green suite is NOT device-correct.** Tests can't see device-data-shape bugs (chain rendering, scene capture, footswitch tags). State explicitly what was verified on real hardware vs simulated, and never claim HW behavior you didn't observe.
4. **Slot-keyed destructive op (clear/move/save-over) → non-destructive read FIRST**, and put the guard in the SAME address space as the mutation (list-index vs 1-based device slot — an off-by-one here deleted a real preset).
5. **Before saying "done":** run the five gates (Commands below), re-walk the original ask, and verify each multi-file edit actually landed on disk (`grep` a distinctive symbol — a concurrent editor save can silently clobber a write).

## Load the right guidance

| You are touching / asking | Read |
|---|---|
| `src-tauri/src/{proto,session,backup,hid,monitor}.rs`, wire bytes, handshake | skill `tmp-companion-protocol` + `notes/protocol.md` |
| `src-tauri/src/commands/**`, `probe_api/**`, `bootstrap.rs` (no skill auto-fires here) | skill `tmp-companion-protocol` + `notes/write-safety.md` |
| Anything in `src/` (views, primitives, invoke wrappers, tests) | skill `tmp-companion-frontend` |
| `src/models/**`, catalog rows, block art data, CPU table | skill `tmp-companion-catalog` |
| "What does a preset/scene/footswitch/template *mean*?" | skill `tmp-companion-data-model` |
| Leveling algorithm / measurement | `notes/leveling.md` |
| Any write that persists to the unit | `notes/write-safety.md` |
| Copy-tab / live block edits | `notes/block-copy.md` |
| Songs/setlists semantics | `notes/songs-setlists.md` |
| e2e harness, test strategy | `notes/e2e-test-plan.md` |
| Clicking the real native window | `notes/native-window.md` |

## House rules (hard IF-THENs — these cannot wait for a skill to load)

**Device safety:**

- **Serialize every device command.** The TMP is single-connection exclusive-HID; two concurrent device ops collide (`0xe00002c5`). Backend enforcement is the process-global `DEVICE_OP_LOCK` inside each command's `spawn_blocking`; frontend must still `await` device calls sequentially — never `Promise.all` two device reads. (why: front-end serialization alone can't stop two *operations* overlapping across a tab switch.)
- **Never re-engage re-amp on a held connection** (disengage → settle → re-engage). Fresh-connect per engage; proof of engagement is a finite captured loudness, never the `ReAmpModeChanged` echo. (why: a held re-engage wedged the device and USB-crashed the whole Mac.)
- **Any slot-keyed destructive op: confirm the mapping with a non-destructive read first, guard in the same address space as the mutation.** Device userSlot = list index + 1; `session.rs` owns the +1. (why: a wrong-space guard silently deleted a real preset.)
- **Never save on `presetError` or an unconfirmed live edit.** Every structural edit (`insertNode`/`replaceNode`/`removeNode`) must be confirmed on its ack (`nodeInserted`(33)/`nodeReplaced`(40)/`nodeRemoved`(36)) before `saveCurrentPreset`. (why: a wrong-content save corrupted a real slot.)

**Cross-language parity:**

- **Amp-id matching is check-first-then-strip, kept in lockstep between `src/models/blockArt.ts::resolveDeviceId`/`resolveCatalogId` (TS) and `src-tauri/src/probe_api/scene_jobs.rs::is_amp_model_id` (Rust).** Suffix set `ConvRvb|CabIR|NoCab|Cab|IR` + one `NoFx` append-recheck; never greedy-strip (reverb amps are catalogued WITH the suffix). Change one side → change both. And the normalization is FRONTEND-ONLY: the backend matches by EXACT FenderId, so a normalized id passed into a backend op silently fail-matches.

**Frontend standing rules:**

- **Every line-icon routes through the `Icon` catalog** (`ui/Icon.tsx` + `ui/iconNames.ts`); no inline glyph SVG in views, no raw emoji as icons. Guard: the `ICONS.length` count oracle in `src/__tests__/DSPrimitives.test.tsx`. Block art routes through `BlockArt` — never `<img>` photo tiles (Fender IP).
- **Optimistic UI, ONE startup read.** All device tabs render from the single App-owned startup backup scan (`ensureLibraryScan()`, module store in `src/views/level/libraryScan.ts`); never add a per-tab re-scan or a post-write device re-read. Sole exemption: the Songs tab's read-back-after-write CRUD (the unit owns songs).
- **Dialogs/overlays go through the ONE DS `Dialog`** (`ui/Dialog.tsx`); anchored menus through `ui/Menu`. Never roll a per-view `position:absolute` scrim (it clips below the tab bar — a real 4-dialog bug).

**Repo:**

- Public MIT repo — commit as `cavpedro@gmail.com` (not a work identity). Signed+notarized DMGs ship automatically via semantic-release on push to `main`.
- No `.claude/skills/...` or other machine-local paths in PRs, commits, or code comments.

## Commands

Run from the repo root:

```bash
bun install                                   # first-time deps
bun run tauri dev                             # launch the app
bun run test                                  # frontend Vitest
bunx tsc --noEmit                             # frontend typecheck
bun run lint                                  # strict eslint (--max-warnings 0)
bun run format                                # prettier --write (before calling a change "done")
bun run build                                 # vite production bundle

cd src-tauri && cargo test --lib              # Rust unit tests (proto golden vectors, lufs, leveller)
cargo clippy --all-targets                    # lint
cargo run --bin gen_samples                   # regenerate resources/samples/*.wav (deterministic)
cargo run --bin probe                         # headless HW re-validation kit (see notes/leveling.md + --help)
bun run e2e                                   # Playwright offline (SimDevice); `bun run e2e online` = real device
```

**Fresh-clone / worktree traps:** `cargo {test,clippy,build}` runs `tauri-build`, which panics if `./dist` is absent (gitignored) — run `bun run build` (or stub `dist/index.html`) first. Run `bun install` before `bunx tsc`/`bun run test` (else hundreds of phantom "Cannot find module 'react'" errors).

**Strict lint + typecheck:** the flat config is `strictTypeChecked` + `stylisticTypeChecked` (type-aware) + `eslint-plugin-react`. **No escape hatches anywhere in `src/`** — no `eslint-disable`/`@ts-ignore`/`@ts-expect-error`, no `any`, no non-null `!`; fix findings by changing code. The trap catalog (hooks rules, `no-unnecessary-condition`, the one legit DOM-measurement exception) lives in the `tmp-companion-frontend` skill.

**Shell scripts must survive bash 3.2** (macOS `/bin/bash`): under `set -euo pipefail` an empty-array `"${arr[@]}"` ABORTS — guard with `"${arr[@]:-}"`, a length check, or `"$@"`. Test under `/bin/bash` explicitly, not the dev's PATH bash.

**Formatting — don't reflow `main`:** neither `cargo fmt` nor prettier is clean on `main`, and the gate doesn't check them — format only files you touched; revert pure-format churn on untouched files before committing. Single-file `rustfmt <file>` needs `--edition 2021` for `async fn`.

**Test infra:** RTL `waitFor`/`findBy` hang forever under vitest fake timers — use real timers. jsdom lacks `localStorage` (shimmed in `src/__tests__/setup.ts`). Full patterns: the frontend skill's `references/testing.md`.

**IDE/editor:** the LSP emits stale phantom diagnostics during rapid file moves or concurrent external edits — `bunx tsc --noEmit` is authoritative; trust the CLI over live squiggles mid-refactor.

**Git:** local `main` is a single squashed root, pushed as `origin/main` + tag `v0.1.0-beta.1`. `git ls-remote` is authoritative for remote state (tracking refs go stale). A background job's worktree-isolation guard can be opted out via `.claude/settings.local.json` `"worktree": {"bgIsolation": "none"}`.

## Architecture

```
src-tauri/src/
  hid.rs          IOKit exclusive-seize HID on a dedicated CFRunLoop thread; hand-declared
                  externs; bounded open-retry + re-enumeration lanes. (Highest-risk module.)
  watcher.rs      Non-seizing hotplug watcher → tmp://device-{attached,detached} events.
  dock.rs         Dev-mode macOS Dock icon (raw objc; rounding baked into dock.png).
  proto.rs        Hand-rolled FenderMessageTMS codec; byte-exact vs golden vectors.
  session.rs      Handshake + device commands; owns the 0-based→1-based slot translation.
  monitor.rs      The live ~250 ms-heartbeat session + startup snapshot + pause/ack seize sharing.
  audio.rs        cpal (CoreAudio AUHAL): re-amp playback into USB-In 3 + capture of USB-Out.
  lufs.rs         ebur128 integrated (gated) + short-term-max; validated vs pyloudnorm ≤0.04 LU.
  leveller.rs     Leveling seams: measure_c / solve_level / apply_level + level_setlist.
  topologies.rs   Pickup-topology catalog (stimulus synth params); profiles.rs = instrument
                  profiles + per-slot assignment (JSON in the app config dir).
  library.rs      OFFLINE .preset folder ingestion + decode_preset_bytes + device reconciliation.
  preset_io.rs    OfflineIo in-place re-import + identity guard; LiveIo diff→changeParameter+save.
  bulk_cmd.rs     Bulk-run engine plumbing (OpSpec + build_operation + IoPath + RunRegistry).
  audiograph.rs   Node ops + the shared node-walk helpers (for_each_node{,_mut}, node_id).
  backup.rs       .preset codec (xor_jld, committed PRESET_XOR_KEY) + backup archive read.
  lib.rs          Slim crate hub (module decls + shared state); the 83 #[tauri::command]s live in
  commands/       one file per domain (device, level, scene_*, songs, setlists, copy_apply, …),
  bootstrap.rs    which owns Builder setup + the generate_handler! list.
  probe_api/      probe entry points + shared helpers (incl. scene_jobs.rs::is_amp_model_id).
  bin/probe.rs    headless re-validation kit; bin/gen_samples.rs stimulus generator;
                  e2e_server.rs (feature "e2e") = the windowless backend for the Playwright harness.
  --- bulk/offline feature modules (probe + test-reachable; NOT in the 5-tab UI; //! docs authoritative) ---
  bulkrun.rs rename.rs paramedit.rs ir.rs blocklib.rs variants.rs migration.rs
  footswitch.rs scenes.rs spectrum.rs search.rs lint.rs presetmeta.rs audition.rs

src/
  theme/          tokens.ts (LIGHT-only tokens) + styles.ts buildStyles(t) + ThemeContext
                  (useTheme()→{t}, useStyles()). Pattern: const { t } = useTheme(); const s = useStyles();
  ui/             Icon.tsx + iconNames.ts (line icons) · BlockArt.tsx + blockart/ per-family files
                  (procedural SVG block art; blockColors.generated.ts is generated — never hand-edit)
                  · primitives.tsx (Button/Slider/Modal/…, deliberately ONE file — circular-import risk)
                  · Dialog.tsx (the ONE DS dialog shell) · Menu.tsx (the ONE anchored menu)
                  · ActionBar · ProgressBar · Skeleton · DeviceStatus · ErrorBoundary · log.ts.
  lib/            invoke.ts (typed wrappers) · types.ts (hand-written mirrors of Rust structs)
                  · format.ts · useDeviceLoad.ts · connectError.ts (connect-failure classifier).
  views/          Feature folders level/ · copy/ · songs/ · settings/ · overlays/ (one component
                  per file + barrel index.ts); flat CatalogView / PresetList / SignalChainView /
                  ActiveSignalChainView / EmptyState. App.tsx routes the 5 tabs.
  models/         catalog.ts (ingest + taxonomy) · blockArt.ts (id→art + nodeTileArt) · lineage.ts
                  · cpu.ts + model-cpu.json · tmp-model-guide.json + blockArtCatalog/ (one file per
                  category). Import direction: catalog→blockArt only (the reverse closes a
                  module-init TDZ cycle); form+art decisions resolve at the VIEW call site.
resources/samples/  7 committed per-topology shaped-noise WAVs (regen: cargo run --bin gen_samples).
```

Key frontend structure facts (depth: the `tmp-companion-frontend` skill):

- **`views/level/`** = `LevelView` orchestrator + per-concern hooks: `usePresetData` (subscriber of the ONE shared `libraryScan` backup store), `useLiveDevice` (module-scoped store over the 5 `tmp://` monitor events — NOT component state; a tab-switch remount would revert the hero to the stale connect-time preset), `useLevelingFlow` (the wizard state machine) + `leveling.ts`. The list is a scene tree (`PresetRow`/`SceneRow`: Base `p${slot}`, scenes `s${slot}:${i}`, levelable footswitches `f${slot}:${i}`); row click = SELECTION only — app-driven preset recall was removed on purpose (recall belongs to Pro Control / the footswitches).
- **The leveling wizard** (`views/overlays/`) is ONE persistent `WizardShell` frame with a 3-step rail **Set up · Level · Summary**; the backup acknowledgment is an inline checkbox in the Set-up footer, not a stage. Run auto-advances to Summary after a natural finish; Stop fires the three `cancel_*_leveling` lanes (AtomicBools → the leveller's `CANCELLED` sentinel = skip). Summary is reason-aware ("by ear" chips: dynamic spread ≥6 LU, rebalance approximation); `clamp_reason` means ONLY "no signal on USB 1/2" (off-branch) — headroom clamps are reason-less.
- **`views/copy/`** = CopyView state machine (ChoosePresets → PlaceBlocks → save via `copy_apply`); `copyModel.ts` owns the editable EditGraph + edit→op diff (`diffToOps`). CopyPath has no renderer of its own — it adapts an EditGraph onto the one `SignalChainView` strip engine.
- **The strip mirrors the unit:** half-stack/dual-cab CREATE decisions key on device `cabsimid` presence, never on catalog `form` — except `form` may SUPPRESS a stack for combo amps (`isComboBid`, threaded as the REQUIRED `isCombo` arg of `nodeTileArt`). Never resurrect form-keyed cab splits (they drew phantom cabs onto bare heads).
- **Songs tab** is device-backed (read-back-after-write CRUD); its Presets axis is READ-ONLY off the shared backup scan.
- **Contract mirrors:** Rust struct → hand-written TS mirror in `lib/types.ts`; adding a Rust field without the mirror fails silently. `invoke.test.ts` pins the exact `cmd` wrapper count; `liveEvents.test.ts` pins the whole `LIVE_EVENT` registry via `toEqual`. Serde-casing exception: `copy_apply`'s nested keys are camelCase via per-field `#[serde(rename)]` that OVERRIDES the enum-level `rename_all = "snake_case"` — verify wire shapes against the per-field attrs.

## How leveling works (one-shot open-loop — HW-validated; full procedure: `notes/leveling.md`)

`presetLevel` is linear amplitude: `captured_LUFS = 20·log10(presetLevel) + C`. Measure once at a reference level, solve `C`, set the exact final level — no iteration. Per preset, three fresh connections (load / measure / apply — the re-amp latch rules force the split). `C` is the preset's max reachable loudness; a louder target clamps (surfaced honestly in the UI).

Order is load-bearing: `presetLevel` is a GLOBAL multiplier over all scenes → level the **base scene first**, then each footswitch scene via its active amp's `outputLevel` (the only permitted per-scene knob — preamp/master/volume alter the sound), then block-acting footswitches one at a time via the chosen block parameter. Footswitch + Base jobs level in ISOLATION (all other block-acting footswitches forced off during measurement; the apply reloads first so forced bypasses are never persisted). Scene-mode (`SetNodeSceneEdit`) writes are per-scene isolated; enable+confirm scene mode BEFORE the value write (a write racing ahead lands on Base).

Instrument-aware: a profile links a real instrument to a pickup topology whose stimulus WAV drives the chain; Tier-2 calibration captures the dry instrument's K-weighted LUFS (USB-Out 3 — which has NO limiter and clips at 0 dBFS for hot playing) and scales the stimulus to match. Fletcher–Munson playback compensation (`playback_offset_for`) adds a bass-only LU offset below stage volume; probe paths bypass it. Every full measure also reports `dynamic_spread_lu` (short-term-max − integrated); ≥6 LU rows get the "verify by ear" chip — the solver still uses integrated only.

## Gotchas — IF-THEN rules (grouped; depth lives in the pointed-to note/skill)

### Wire protocol (depth: `notes/protocol.md` + the protocol skill)

- **Sending any request burst:** mirror Pro Control's exact `batchStatus` grouping (1 / 2×7 / 3 / 4) — incrementing per request makes the device go silent after the preset lists. **Setters + the heartbeat OMIT `batchStatus`** — a setter carrying one is silently ignored.
- **Slot addressing:** `list_my_presets` is 0-based; every slot-addressed setter is 1-based (`session.rs` owns the +1; callers pass list indices). Empty-slot marker is `--`/"Empty"; `requestNextEmptyPresetSlot` (81→82) is dead on 1.7.75.
- **Reading preset JSON:** there is NO reliable complete-preset read over USB — field-3 (`currentPresetDataChanged`, LZ4, session-health-dependent partial), field-8→9 (plaintext slot-addressed partial, no `batchStatus`, quiet-line + `connection_request` re-arm; on an already-live heartbeat session use `read_slot_preset_json_live` which SKIPS the re-arm), and field-115 (no companion-replayable reply) are all partial. **The canonical full-preset source is the OFFLINE `.preset` file** (`notes/write-safety.md`). The device answers exactly ONE data request per burst state; a read fired mid-flood is dropped.
- **A PC-style rename is rename(13) + save(14)** — send both to persist; `moveUserPreset` persists alone. Full-preset import = `importPresetRequest(117)` with `LZ4(raw .preset bytes)`; outbound multi-packet framing `0x33/0x34/0x35`.
- **Firmware version** = in-burst `currentFwRequest`, no batchStatus, inside the batch-2 group (after `userir_field2`, before batch-3) — anywhere else the reply is dropped.
- **Song-preset reads reply only in-burst with a top-level `batchStatus`**; song CRUD semantics (insert-at-slot-1 shift, 1-based setlist positions, BPM via `tapTempoBpm`): `notes/songs-setlists.md`.
- **Preset-list reassembly** needs both stream rules (`streams()` + `streams_final()`); tolerant longest-wins can still accept a TAIL-truncated list, so the startup snapshot uses `list_my_presets_strict` (+2 re-arm retries, warn-don't-fail). Leveller/probe stay on tolerant reads (they shape their own bursts).
- **`PRESET_XOR_KEY` is committed** (`backup.rs`, `*b"JLD"`) — do NOT reintroduce the runtime `derive_key`/`learn_key` recovery (it panicked fresh headless servers).

### Re-amp + measurement (depth: `notes/protocol.md`, `notes/leveling.md`)

- **Re-amp toggle** = `SettingsMessage(3) → reampModeActive(30)`, not MixerMessage. It **latches preset state at engage**: set level → THEN engage. `load_preset` + engage in the same connection captures silence; `load_preset` + `set_preset_level` in one connection → the set is overridden. `changeParameter` IS audible mid-engage; `loadScene` is NOT (one engage per scene).
- **Re-amp engages reliably only ONCE per connection**; never re-engage held (House rules — it rebooted a Mac). Every leveling run ends with a guaranteed re-amp OFF on a fresh connection (a dropped OFF strands the unit input-muted; recovery: `probe --reamp-off`).
- **The 6 s + 0.8 s capture window is LOAD-BEARING** — presets are non-stationary under gated LUFS (reverb build-up + tail), so any capture-shortening is a ≤0.3 LU re-baseline, not a free speedup. Validate measurement changes against the full-capture oracle (`probe --measure-adaptive`), never a self-consistent level→verify round-trip. HW noise floor is ~0.12 LU run-to-run — conclude bias only from many-sample A/Bs. 48 kHz stimulus required.
- **Scene leveling is ONE-SHOT open-loop on amp `outputLevel`** (isolated capture per measurement point). Do NOT rebuild the closed-loop shared-stream approach — its windowed reads mis-measured garbage and it clamps on it (`notes/leveling.md`, capture-stream rules; `level_scenes_live_batched` survives only for the `probe --bench-scene-leveling` harness). Amp candidates: classify by stable FenderId, allow ONLY amp `outputLevel` (preamp/master/volume forbidden).
- **`outputLevel`=0 is deep digital silence** and `loudest_loudness` errors on it — deliberate-mute measurements must treat that error as the sentinel floor (`MUTE_FLOOR_SILENT_LUFS`), never propagate it.
- **`audio::LiveReamp` is ring-buffered** — unbounded capture accumulation once OOM'd the whole Mac; never reintroduce it.
- **Open scene-0 anomaly:** on the 2-amp Guitar preset, USB `loadScene(0)` materializes a different amp state than the physical footswitch tap — don't trust scene-0 leveling until resolved.
- **Wire scene addressing:** `loadScene`/`lastLoadedScene` are 0-BASED `scenes[]` indices; base = CONSTANT slot 8; `loadScene` must emit `sceneSlot` explicitly even for 0.

### Live block edits (depth: `notes/block-copy.md` + `notes/write-safety.md`)

- **Any block mutation touches THREE differently-keyed places** — roster (`guitarNodes`/`micNodes`, by FenderId), `scenes[].<group>.<FenderId>` overrides, `ftsw[…].nodes[].nodeId` — or leaves dangling state (`drop_scene_overrides`/`retarget_ftsw` are the shared helpers). Confirm every edit on its ack; never save unconfirmed (House rules). First edit after a fresh load can be dropped — retry once.
- **Saved-block/IR inserts are LIVE-ONLY** (saved blocks are metadata-only; there is no offline payload to reconstruct — don't re-attempt an offline splice).
- **Post-edit reads don't reflect the edit on the held session** — verify placement via a post-save field-8 read, not an in-session graph re-read.

### Connection lifecycle (depth: `notes/protocol.md`)

- **Exclusive seize blocks Pro Control and vice versa** (`0xe00002c5` → "close Pro Control" red banner). The same code fires on concurrent commands (House rules: serialize) and during the post-close **open-lockout**: quick re-open ≤~800 ms works, then tens of seconds of lockout where every failed attempt RESETS it — `hid.rs` retries same-ref fast (6×80 ms) then re-enumerates (3×8 s quiet). A `probe` hang with zero output = an open landed in the lockout; kill, wait, retry once.
- **Boot-window `IOHIDDeviceSetReport failed: 0xe00002d6` is "device not ready yet"**, not an error (~20 s cold-boot window) — `connectError.ts` classifies by error STRING (rewording a backend HID error must update the matcher).
- **Connection is fully automatic** — no Connect button; App retries `connect()` every 3 s until found, `watcher.rs` detach events reset state, replug auto-reconnects. `connect_device` releases any old seize, enables the monitor, and serves the monitor's `StartupSnapshot` (firmware + strict preset list + graph); it's idempotent vs an already-running monitor (serves the cached snapshot — never reset it, the one-shot handshake can't re-run). `graph=none` snapshots self-heal via bounded re-snapshot retries.
- **The full first-connect handshake is load-bearing** — the device only answers after the captured Pro Control sequence. Don't trim it without HW testing.
- **`connect_for_discovery` (field-78) is effectively DEAD on fw 1.8.45** (kills field-3 for the whole session). Block/graph discovery uses a RICH lean session: heartbeat warmup → `send_and_collect(LoadPreset)` → pump → graph from the accumulated push bodies (`discover_blocks_rich`); field-78 remains only as an older-firmware fallback.
- **Live-session lanes:** the monitor's persistent session serves live ops (`try_live_op`) and metadata reads (`read_slot_preset_json_live`); every non-live state falls back to the classic release→fresh-handshake path. `load_preset_on_amp`/`load_scene_on_amp` + `read_preset_scenes` are UI-ORPHANED (kept as APIs; recall was removed from the UI). Known hotspot: Songs first paint can hit 5.4–9.4 s (10× full-handshake retries).
- **Scene policy is pure-lazy** — no eager startup scene sweep; the LevelDialog batch sweep `scan_preset_scenes` (one lean session, ~0.66 s/preset, NON-DESTRUCTIVE — zero LoadPreset) is the only bulk read; per-preset `listLevelBlocks` (destructive, ~4 s) must NEVER run on dialog open. Device state changes only in the post-disclaimer leveling RUN.
- Heavy rapid open/close cycling congests the TMP (reads slow to seconds until a power-cycle) — don't add churny reconnect loops.

### Frontend (depth: the `tmp-companion-frontend` skill)

- **React hooks precede any conditional early return** (a violation blanked the whole window pre-ErrorBoundary; top-level + per-tab `ErrorBoundary` now defends).
- **`window.confirm()`/`alert()` silently no-op in WKWebView** (`confirm` returns `false`) — never gate logic on them; use inline UI/countdowns.
- **`Pick`/`BlockPick` display `options[0]` when `value` isn't in `options`** — derive defaults from the live option source, never hard-coded ids (a hard-coded default silently levelled to −30).
- **Tauri `core:default` does NOT grant window creation** — a second `WebviewWindow` silently fails without the `core:window:allow-*` capabilities (all removed; Settings is a tab now).
- **Editing `index.html` triggers a FULL page reload** (not HMR) in `tauri dev` — re-runs connect-on-mount; reload via the UI instead.
- **Device tabs refresh on the `connected` flag flipping true** (not mount-only) — plugging in later auto-populates.
- **Device-returned strings are NOT unique** (duplicate IR/preset names) — never use one alone as a React key.
- **Logging:** `tauri-plugin-log` → `~/Library/Logs/dev.cavadas.tmp-companion/`; `.clear_targets()` before re-adding targets (else double-logging). Frontend errors route via `ui/log.ts` (guarded by `isTauri()`). Backend uses `log::*`; the CLIs keep `println!`.

### e2e + hardware validation (depth: `notes/e2e-test-plan.md`, `notes/native-window.md`)

- The dual-mode Playwright harness (`bun run e2e` / `bun run e2e online`) drives the real React UI in headless Chromium → HTTP bridge → windowless Rust backend → `SimDevice` offline / real device online. Always use `scripts/e2e.sh`, never raw `playwright test` — it kills stale `:7600` servers (the FAKE-ONLINE trap: a stale offline server makes the "online" suite pass green without touching the device) and recovers the unit on exit. Online scenario presets live at list indices 400/401/402 and are cleared in teardown; the offline `backup-fixture.bin` and `scenario-presets.json` must stay in sync (regen both from one script).
- `scripts/hw-e2e.sh` = attended, non-destructive on-device happy paths. `probe` subcommands are the HW re-validation kit (`--lufs`, `--levelpreset`, `--device-backup`, `--reamp-off`, …).
- Driving the literal native window (cliclick/screencapture, incl. the locked-screen and stale-dev-server signatures): `notes/native-window.md`.

### Assets / release

- **App icon:** flat terracotta squircle + 3 white level bars; macOS `.icns`/`dock.png` are FULL-BLEED baked squircles (no inset — that constraint was waived); vector masters in `icons/`; `src/__tests__/icon-assets.test.ts` pins shipped sha256s — recompute on any bundle change. iOS slots are full-bleed opaque mask-free; Android adaptive + monochrome layers.
- **Marketing site** = `docs/` on GitHub Pages (project subpath → all asset paths RELATIVE; `docs/` is web-published, so dev docs live in `notes/`). Releases: semantic-release, signed+notarized, stable asset name `TMP-Companion-AppleSilicon.dmg`, Apple-Silicon-only — don't reintroduce Intel copy or `xattr` workarounds.
- **macOS-only:** IOKit + cpal CoreAudio deps are `cfg(target_os = "macos")`.
- **Saving permanently alters a preset** — persistence is opt-in everywhere (the checkbox is the only gate); revert/backup of the original `presetLevel` is still TODO.

## Notes & skills index

- `notes/overview.md` — human onramp (tabs, data paths, platform constraints).
- `notes/protocol.md` — read before ANY wire/session/HID change.
- `notes/write-safety.md` — read before ANY write that persists to the unit.
- `notes/leveling.md` — the leveling algorithms + capture-stream rules.
- `notes/block-copy.md` — the Copy tab + live structural edit protocol.
- `notes/songs-setlists.md` — song/setlist wire semantics + positional traps.
- `notes/e2e-test-plan.md` — the test matrix, harness how-to, live-run order.
- `notes/native-window.md` — clicking the real window from a Claude session.
- Skills: `tmp-companion-protocol` (wire codec/handshake how-to), `tmp-companion-frontend` (React/DS/test conventions), `tmp-companion-catalog` (catalog data contract), `tmp-companion-data-model` (what device concepts mean).
