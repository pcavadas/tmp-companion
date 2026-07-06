# TMP Companion — Pre-release deep E2E + monkey-test plan

> **Status: point-in-time discovery snapshot (2026-06-30).** The scenario inventory is still
> useful; §8's doc-correction items are resolved unless marked open — do not treat them as
> current bugs.

Discovery-only artifact (no implementation). Maps **every** interactive surface, state
machine, edge state, and chaos sequence across the 5 tabs, consolidates them into a runnable
scenario set, and tags each scenario with a regression-automation recommendation.

Sources: a full read of `src/views/**`, `src/App.tsx`, `src/ui/{DeviceStatus,ErrorBoundary}`,
`src/lib/{connectError,gates,firmware}`, the existing `e2e/specs/**` + `src/__tests__/**` +
`src-tauri/src/*` tests. Corrections to CLAUDE.md found during discovery are in §8.

---

## 0. How to run

`scripts/e2e.sh` is the turn-key runner (also `bun run e2e`):

- **`bash scripts/e2e.sh`** — OFFLINE, all specs against the SimDevice (fast, ~1.5 min, no hardware).
- **`bash scripts/e2e.sh online`** — ONLINE against the real unit (songs → copy → level, one at a
  time, ~9 min). Preconditions: the unit plugged in + **rested**, and **Pro Control closed** (it
  holds the exclusive HID seize). The script pre-flights the handshake, runs each spec in its own
  invocation, and ALWAYS recovers the unit on exit (reamp-off + guarded scratch-clear 400/401/402 +
  recall 001) — even on Ctrl-C or a killed run.

Add `copy` / `level` / `songs` to run a single spec (e.g. `bash scripts/e2e.sh online level`).

The rest of this doc is the **coverage map** — the per-tab scenario inventory + the still-un-automated
backlog (§9), independent of how the suite is launched. Markers below: **⌨️** = the scenario needs
text entry, **🖱️** = needs drag/DnD (both are exercised fine by the Playwright harness; only the
native-window `cliclick` path can't type — relevant just for a manual Claude-driven sanity pass).

**Safety net:** every destructive scenario (leveling-save, Copy save, Songs delete/rename) is
recoverable via a Pro Control backup restore; run destructive scenarios **last** so a restore is a
single rollback at the end.

---

## 1. Test matrix at a glance

| Area                 | Functional scenarios | Monkey/chaos scenarios | Existing auto-coverage                            | Biggest gap                                                         |
| -------------------- | -------------------- | ---------------------- | ------------------------------------------------- | ------------------------------------------------------------------- |
| Connection lifecycle | C1–C7                | M-C1–M-C3              | unit (`connectError`, `DeviceStatus`, `firmware`) | mid-op disconnect; Pro-Control-grab red banner live                 |
| Level + wizard       | L1–L14               | M-L1–M-L8              | e2e happy paths + many vitest                     | **cancel mid-run**, partial failure, clamp/offbranch live           |
| Copy                 | P1–P10               | M-P1–M-P5              | e2e + vitest happy paths                          | save-abort, partial save failure, Back-discards-edits               |
| Songs                | S1–S13               | M-S1–M-S4              | e2e CRUD + vitest                                 | **add-to-setlist + reorder e2e**, BPM-didn't-stick warn             |
| Catalog              | T1–T7                | M-T1–M-T2              | model-wall + `CatalogView` interaction unit tests | scroll/DnD only (filters/sort/facets now in `CatalogView.test.tsx`) |
| Settings             | E1–E10               | M-E1–M-E3              | vitest only                                       | **calibration flow live**, slider/drag, no e2e at all               |

---

## 2. Connection & app-shell lifecycle

Preconditions reset between runs by clearing `localStorage`/`sessionStorage` (the disclaimer +
gate keys live there — `gates.ts`).

- **C1 — Cold launch, device present.** Launch app → expect Disclaimer gate first (storage clear),
  then after accept: DeviceStatus goes hollow→amber "reading firmware…" (≥900 ms floor)→green
  "connected · <fw>". Level tab paints list + hero together.
- **C2 — Disclaimer gate.** Both paths: "I've backed up — continue" with and without "Don't show
  again". Verify the perm checkbox writes `localStorage tmp_disclaimer_perm=1` (survives relaunch);
  without it, only `sessionStorage` (re-prompts next cold launch).
- **C3 — Launch with no device.** Unplug → launch → friendly "Please connect your Tone Master Pro…"
  banner (NOT red), DeviceStatus hollow, 3 s retry loop armed. Plug in → auto-connects within ~3 s,
  no "Try again" needed.
- **C4 — Pro Control holding the device.** Open Pro Control, launch app → **red** AlertBanner with
  the "close Fender Pro Control" hint (the one actionable-error case). Close PC → retry/auto-recovers.
- **C5 — Firmware gate.** (Hard to force without old fw.) If fw < 1.7: Level/Copy/Songs show
  FirmwareGate full-page ("Check again" / "Use it anyway"); Catalog/Settings stay reachable;
  DeviceStatus shows persistent "untested ⚠". "Use it anyway" unlocks per-mount.
- **C6 — Hot-plug detach.** While connected on Level, unplug → DeviceStatus→hollow, body→EmptyState,
  firmware/graph cleared, retry loop stays armed. Replug → auto-reconnects, body repopulates.
- **C7 — Tab routing.** Click through all 5 tabs; each body wrapped in `ErrorBoundary key={tab}` —
  a crash in one tab shows its fallback ("Try again"/"Reload") without killing the tab bar or
  connection. Catalog/Settings render disconnected (deviceIndependent).

**Monkey:**

- **M-C1 — Disconnect mid-operation** (the #1 gap). Pull USB during: an active Level run, a Copy
  save, a Songs write. Expect graceful failure (Toast / error item), no hang, no double-write on
  replug. _Not covered by any test._
- **M-C2 — Rapid replug** (attach/detach spam) — verify the single-flight `attempt()` guard and the
  3 s interval don't stack connects (would collide → `0xe00002c5`).
- **M-C3 — Reload during handshake** (native: editing index.html / Cmd-R equivalent) — idempotent
  reconnect serves the cached snapshot, doesn't re-run the one-shot handshake or hang 8 s.

---

## 3. Level tab + leveling wizard

The wizard is **3 stages** — `setup → run → summary` (no separate Disclaimer/Backup stage; the
backup ack is an inline checkbox in the Setup footer, see §8). Most monkey-value is concentrated here.

### Functional

- **L1 — Hero diagram.** Active preset renders in ActiveSignalChainView; scene tag + slot badge
  correct. Skeleton on load; "Scene recalled — signal view didn't refresh" + Retry on the watchdog.
- **L2 — Preset list paint.** List populates on connect rising-edge (plug in after launch still
  populates). Backup-scan strip shows determinate %; carets inert until `ready`.
- **L3 — Selection model.** Whole-preset checkbox selects ALL children (Base + FS scenes +
  footswitches). Indeterminate when 0<sel<total. Select-all header checkbox + indeterminate.
- **L4 — Scene tree.** Expand caret reveals Base / FS-scene / footswitch child rows; "N scenes ·
  M footswitches" breakdown collapsed; toggle individual child keys; empty slot = inert "— empty —".
- **L5 — Filter** ⌨️ — type a name; selection persists across filter changes; input disabled while
  loading.
- **L6 — "How leveling works" sheet** — opens, Got it / X / backdrop / Escape all close it.
- **L7 — Setup: apply-to-all** — bulk Instrument + Target pickers brush all ticked rows; "Clear
  ticks"; per-row overrides (Instrument / Target / FsParamPick).
- **L8 — Setup: backup ack gating** — "Level N sounds" disabled until backup checkbox ticked (fresh
  run only; re-level hides it). Disabled at total=0.
- **L9 — "Even out parallel amps" toggle** — default off; on → ByEarChip + rebalance footnote band.
- **L10 — Run: whole preset** — fires `level_preset` + `level_scenes_apply_batched` +
  `level_footswitches_apply`; live LUFS strip; auto-advance to Summary 650 ms after natural finish.
- **L11 — Run: Base-only / scene-only / footswitch-only** — verify only the relevant command(s)
  fire (footswitch-only dispatches `level_footswitches_apply`, not `level_preset`).
- **L12 — Summary: all-good** — "All N sounds leveled", green, "Done" (clears selection).
- **L13 — Summary: clamped** — clamped group + amber "lower the target" banner + **Re-level clamped
  subset** (→ Setup, re-level mode, backup ack hidden). Off-branch → "Needs routing", never offered
  re-level.
- **L14 — By-ear footnotes** — dynamic (spread ≥6 LU) and rebalance reasons; one chip per row;
  closing line spells out only the causes present.

### Monkey/chaos (the high-value gaps — near-zero existing coverage)

- **M-L1 — Stop mid-run, then Continue.** Stop during scene 2 of 3 → all three cancel lanes fire,
  in-flight ~6 s capture finishes, "Stopping…"→"Leveling stopped", remaining items "Not leveled",
  **already-written levels stay saved**, Continue → Summary. **Verify the unit is NOT left
  input-muted** (the reason `clearScenario` calls `e2e_reamp_off`). _No UI test exists for this._
- **M-L2 — Stop then Re-level the partial.** After a stopped run, re-level the clamped/leftover set.
- **M-L3 — Cancel → reopen → run again.** Re-issues commands fresh (no stale cross-run cache —
  vitest covers the assertion, but verify live).
- **M-L4 — Backdrop/Escape probing per stage.** Setup: no backdrop/Escape (full-page) — confirm
  clicking outside does nothing. Run: backdrop/Escape **blocked**. Summary: backdrop = Cancel
  (closes, does NOT clear selection — asymmetry vs Done). Verify each.
- **M-L5 — Double-click the primary.** Double-click "Level N sounds" / "Done" / "Re-level" — must
  fire once, not start two runs.
- **M-L6 — Tab-switch mid-run / mid-scan.** Switch away during an active run and during the backup
  scan; return. Hero must survive remount (module-scoped store); run state and shared `libraryScan`
  must not corrupt. _Only single remount-with-push is covered._
- **M-L7 — Partial failure.** A preset/scene that errors mid-run → item "skipped", run continues,
  never aborts; Summary shows it. _No test injects a `status:"error"` item._
- **M-L8 — Empty / no-scene / no-footswitch presets** in setup — Base row labeled "Whole preset";
  no caret; FsParamPick column empty for re-leveled footswitches.

---

## 4. Copy tab

All of Step 1 + Step 2 is **offline** (one `list_presets` read + shared backup scan). The **only**
device write is `copy_apply` on Save.

### Functional

- **P1 — Gating order** — disconnected ("Copy lives on the Tone Master Pro") > error ("Try again") >
  steps. Reference auto-defaults to on-unit preset (or `presets[0]`) on late list arrival.
- **P2 — Step 1 reference pick** ⌨️(filter) — single-select radio; picking a from-slot that's in the
  target set auto-removes it from targets.
- **P3 — Step 1 targets** ⌨️(filter) — multi-select; "Select all" / "Select these N" (filtered) /
  "Clear" (disabled at 0); to-list excludes the current from-slot.
- **P4 — "Place the blocks" gating** — disabled until ≥1 target AND a from-slot; shows
  "Reading presets… %" ReadingPill until backup scan `ready`.
- **P5 — Step 2 per-target edit** — tap a tile → inline BlockEditor; 3-way Replace / Insert before /
  Insert after; origin chips (distinct reference blocks, with +cpu); Remove; close ×. Only ONE
  editor open app-wide. + / ⟲ badges; "edited" chip; CpuMeter per card.
- **P6 — CPU budget** — push a target over budget → amber card border + "over budget" chip + Save
  blocked with the right hint ("{name} is over {budget}% — remove a block"); over-budget filter chip.
- **P7 — Undo / redo** — toolbar buttons; disabled at history ends; a new edit truncates the redo
  branch.
- **P8 — Save gate** — "Save to the unit" disabled unless ≥1 edited target + nothing over budget +
  backup ack ticked.
- **P9 — Save run** — SaveOverlay streams per-slot status (active spinner / queued / done "updated"
  or "no change" / error); ProgressBar; Done medallion + Pro-Control restore note → resets to Step 1.
- **P10 — Multi-preset save** — two targets, different edits each; both written; re-open shows edits
  carried via optimistic cache patch (no second 22 s scan; `read_library_via_backup` called once).

### Monkey/chaos

- **M-P1 — Back discards staged edits.** Edit blocks in Step 2 → Back → "Place the blocks" again →
  edits silently gone (fresh `initEdit`). Confirm this is the actual behavior and decide if it's a
  bug (likely a UX trap worth a guard/warning).
- **M-P2 — Save-abort / disconnect mid-save.** Pull device during a 2-preset save → partial results;
  what does the optimistic cache do on partial failure? Done screen still reached on full failure
  (catch sets `saveDone`). _Untested._
- **M-P3 — Partial save failure** — one of N presets returns `error` → row shows warn-tri, others
  "updated", only updated items patch the cache. _No test injects an error item._
- **M-P4 — Double-click Save** — must fire `copy_apply` once.
- **M-P5 — Rapid tab-switch during the backup scan** that gates "Place the blocks" (shared
  `libraryScan` with Level/Songs).

---

## 5. Songs tab

All writes are **device-backed read-back-after-write**, serialized through `runDeviceOp` (single
in-flight, `busy` gates all buttons). The **Presets axis is read-only** (from the startup backup scan).

### Functional

- **S1 — Gating** — disconnected ("Songs & setlists live on the unit") > loading skeleton > error >
  manager.
- **S2 — Rail pivot** — Seg "Setlists ⇄ Presets" toggles `railAxis` and resets view to "all".
- **S3 — Create song** ⌨️ — New song → SongForm (name/BPM/notes); read-back returns fresh list.
- **S4 — Edit song** ⌨️ — diffs changed fields only; **no-change edit issues no device op**.
- **S5 — Delete song** — confirm Modal ("DESTRUCTIVE · WRITES TO THE TONE MASTER PRO"); Cancel vs
  Delete; removes from unit + every setlist.
- **S6 — BPM validation** ⌨️ — strips non-digits, clamps 20–400; blank → null.
- **S7 — Create setlist** ⌨️ — inline name input (Enter commit / Esc / check / ×); empty name no-ops.
- **S8 — Rename setlist** ⌨️ — click title → inline input; commits on blur AND Enter; empty/unchanged
  → no op.
- **S9 — Delete setlist** — ⋯ menu or detail; confirm Modal; keeps songs, drops the membership entry.
- **S10 — Add songs to setlist** — AddSongs popover ⌨️(filter); multi-select; "Add {n}" (disabled at
  0); batched multi-add; read-back members. _(e2e currently only creates/deletes an EMPTY setlist —
  add-to-setlist is uncovered.)_
- **S11 — Create-and-add** ⌨️ — from AddSongs footer; create + add in one batched txn; duplicate-name
  edge returns `members=null` → cache cleared, re-read on next select.
- **S12 — Reorder setlist songs** 🖱️drag — grip drag-reorder (1-based positions); no-op on same pos.
  _Needs DnD events (not plain clicks); e2e-uncovered._
- **S13 — Remove from setlist** — × on member row; 1-based position.
- **S14 — Presets axis (read-only)** — songs-per-preset counts; PresetDetail non-interactive;
  "managed on the unit in Pro Control" note. Counts show `—` until first read.

### Monkey/chaos

- **M-S1 — Rapid CRUD spam** — fire create/delete/rename back-to-back; `busy`/`busyRef` must
  serialize (single in-flight), buttons disable; no concurrent USB connect collision.
- **M-S2 — Write-failure Toast** — induce a failed write (e.g. disconnect mid-op) → err Toast,
  **state NOT mutated** (read-back failed, UI keeps prior authoritative list). _vitest covers the
  mock; verify live._
- **M-S3 — BPM "didn't stick" warn** — the non-fatal warn Toast path ("Saved, but BPM didn't
  stick"); `bpm_warning` is always null in tests → entirely unexercised. Try a BPM the firmware
  rejects.
- **M-S4 — Delete the currently-viewed setlist** — view drops to "all" cleanly; no dangling
  membership read.

---

## 6. Catalog tab (device-independent — works disconnected)

### Functional

- **T1 — Search** ⌨️ — matches model / real-unit / brand; clear X; count readout updates; empty
  "No models match" / "…these filters".
- **T2 — Category rail** — All models + per-category + Effects disclosure (caret opens subcategories;
  note: no collapse path once opened — see §8); counts per row; scroll the rail.
- **T3 — Effect-type chips** — appear inside an Effects subcategory with >1 type; "All {sub}" +
  per-type toggle.
- **T4 — Facets** — Mono / Stereo (disabled when no stereo in scope — can disable mid-session as
  scope changes) / Convolution (only in Reverb/Effects scope).
- **T5 — Sort** — Type / CPU (label cycles CPU → CPU ↓ → CPU ↑; grouping changes to ranked wall).
- **T6 — Model card select** — click → detail bar populates (type path, real unit, cpu\*, routing,
  since); hover styling; sticky group headers; scroll the wall.
- **T7 — Block art correctness** — combo vs head vs half-stack tiles; null-bid mics resolve by name
  (regression-prone — `models-catalog.test.tsx` is the guard).

### Monkey/chaos

- **M-T1 — Rapid facet toggling while Stereo self-disables** — toggle routing=stereo then narrow
  scope so `hasStereo` flips; button greys mid-interaction; list still filters correctly.
- **M-T2 — Search + filter + sort combinations** — search while CPU-sorted; clear into a filtered
  category; scope changes drop `convOnly`/`et` appropriately.

---

## 7. Settings tab (device-independent except calibration; **no e2e coverage at all today**)

### Functional

- **E1 — Loudness targets render** from store; empty "No targets yet".
- **E2 — Add target** — appends "New target" @ −22, opens in rename mode ⌨️.
- **E3 — Rename target** ⌨️ — double-click name → input; Enter/blur commit, Esc cancel;
  empty/whitespace reverts to old.
- **E4 — Target slider** 🖱️drag — pointer drag, bounds TMIN −32 / TMAX −16, 0.1 steps, commits once
  on pointerup; no upper ceiling beyond track domain. _Needs pointer events._
- **E5 — Reorder targets** 🖱️drag — HTML5 DnD grip; disabled while name-editing; no-op on self.
- **E6 — Target ⋯ menu** — Rename / Delete; delete-last → empty state.
- **E7 — Playback level** — SegmentedControl Quiet/Rehearsal/Stage; persists; comp caption (bass
  +1.5 / +0.5 / no-comp).
- **E8 — Add instrument** ⌨️ — disabled if topologies empty; InstrumentForm (name ⌨️ + type chips +
  pickup chips); Save disabled until name+topology valid; type change re-defaults pickup.
- **E9 — Edit / move / delete instrument** — ⋯ menu Edit / Move up / Move down (no-op at bounds) /
  Delete.
- **E10 — Tier-2 calibration** (device) — Calibrate → countdown 3→2→1 (~850 ms) → recording
  "{rec}/8s" + determinate bar; backend result wins the phase; re-reads `calibration_lufs`. Disconnected
  → "Needs device" pill. _No UI flow test anywhere._

### Monkey/chaos

- **M-E1 — Cancel calibration** at countdown vs recording (✕) — `abortedRef` ignores a late backend
  result; timers cleared.
- **M-E2 — Tab-switch away mid-recording** — mounted guard + clearTimers on unmount; no late setState.
- **M-E3 — Drag a target onto itself / delete a target mid-rename / rename to whitespace** — all
  no-op safely.

---

## 8. CLAUDE.md corrections found during discovery

1. ✅ resolved (2026-07-05): **No "Disclaimer/Backup" wizard stage.** The leveling wizard is
   `setup → run → summary` (3 stages). The backup acknowledgment is an **inline checkbox in the
   Setup footer**, not a step. The CLAUDE.md/handoff "4-step rail (Back up · Set up · Level ·
   Summary)" describes the _visual rail_, but there is no separate backup **stage** to navigate to
   — anyone scripting one will not find it. CLAUDE.md now reads "3-step rail (Set up · Level ·
   Summary)", and `WizardShell.tsx`'s `StepRail` matches that 3-node rail — the doc mismatch this
   item flagged is gone.
2. ✅ resolved (2026-07-05): **Row click does not recall the preset.** `LevelView.tsx:8-13` header
   comment says clicking a row "RECALLS the preset"; the actual `PresetRow` only calls
   `onTogglePreset` (selection), matching the `PresetRow.tsx:11-13` comment. The stale comment at
   the top of LevelView contradicts the code. Both files now say clicking does **not** recall the
   preset (recall is owned by Pro Control / the footswitches) — the comments agree with the code.
3. ⚠️ still open (2026-07-05): **Catalog Effects disclosure has no collapse path** — `fxOpen` is
   only ever set `true`; once opened, the subcategory rows can't be hidden again. Minor, but a real
   one-way toggle. Confirmed via grep (`src/views/CatalogView.tsx`): `setFxOpen` is called exactly
   once, with `true` — the behavior described is unchanged.
4. ⚠️ still open (2026-07-05): **Copy "Back" silently discards staged edits** (fresh `initEdit` on
   re-entering Step 2) — a UX trap worth either a guard/confirm or a doc note. Confirmed via grep
   (`src/views/copy/CopyView.tsx`): `onBack` only does `setStep(1)`, and `onContinue`/`enterStep2`
   re-runs `initEdit(toSlots, graphForSlot)` on return to Step 2 — still discards the prior history
   stack.

---

## 9. Consolidation → regression-automation backlog

Mapping the scenarios above to the cheapest durable layer. "Have" = already covered.

| Layer                                                                                   | Add these (highest value first)                                                                                                                                                                                                                                                                                                                               |
| --------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Rust unit** (have: proto/session/leveller/lufs/monitor/library/variants)              | `bpm_warning` propagation path; a `copy_apply` partial-failure (one slot `error`) shaping the streamed items.                                                                                                                                                                                                                                                 |
| **Vitest component** (have: most happy paths)                                           | **Cancel-mid-run RunBody → Summary "Not leveled" + levels-kept** (inject cancel); **partial-failure item** (`status:"error"`) in Level _and_ Copy streaming; **double-click guard** on Level/Copy/Songs primaries; BPM-warn Toast render; Copy "Back discards edits" assertion; Settings calibration phase machine (countdown→recording→abort, `abortedRef`). |
| **Playwright offline** (have: copy/level/songs happy paths + `CatalogView` interaction) | **Add-song-to-setlist + reorder + remove** (current songs spec only makes an empty setlist); empty-library Copy/Level UX; backup-scan-failure rendering; tab-switch-during-scan. Settings spec (none exists); a Catalog Playwright spec is optional (the vitest one covers filters/sort/facets).                                                              |
| **Playwright online** (have: all 3 specs GREEN online, attended)                        | The genuinely hardware-only ones: **Stop-mid-run leaves unit un-muted**; clamp/off-branch on a real hard target; cold-boot SetReport window; Pro-Control-grab red banner; ~22 s backup latency. Most overlap with the manual run order in §10.                                                                                                                |
| **Manual / Claude-driven only** (cannot reasonably automate)                            | Real LUFS convergence accuracy; native-window chrome (Dock icon, focus, OS dialogs); `cliclick` left-display caveat; physical replug timing.                                                                                                                                                                                                                  |

---

## 10. Proposed live-session run order (when we execute)

Non-destructive first, destructive last (single backup-restore rollback at the end):

1. **Read-only sweep** — C1, C3, C6/C7, all of Catalog (T1–T7, M-T\*), Settings render (E1, E7),
   Level list/scene-tree/hero (L1–L6), Copy Step 1 + Step 2 editing **without saving** (P1–P8, M-P1),
   Songs read + Presets axis (S1, S2, S14).
2. **Monkey on non-writing surfaces** — M-L4/M-L5(open only)/M-L6, M-P5, M-T1, wizard backdrop/Escape
   probing, rapid tab-switching, double-click on gated-disabled buttons.
3. **Destructive, recoverable** — one real Level run + Stop-mid-run (M-L1, the marquee gap) + clamp
   handling (L13), one Copy save (P9/P10) + double-click (M-P4), Songs CRUD + setlist add/reorder
   (S3–S13, M-S1–M-S4), Settings calibration (E10, M-E1/M-E2).
4. **Disconnect chaos last** — M-C1 (pull mid-Level-run / mid-Copy-save / mid-Songs-write), M-C2,
   C4 (Pro Control grab).
5. **Restore** the unit from the Pro Control backup; confirm clean state.

> Text-entry (⌨️) and drag (🖱️) scenarios run fine under the Playwright harness (`scripts/e2e.sh`);
> only a manual native-window `cliclick` pass can't type them.
