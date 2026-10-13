# TMP Companion — Pre-release deep E2E + monkey-test plan

> **Status: point-in-time discovery snapshot (2026-06-30).** The scenario inventory is still
> useful; §8's doc-correction items are resolved unless marked open — do not treat them as
> current bugs.

Discovery-only artifact (no implementation). **Currency rule:** the operational
instructions (runner invocations, preconditions, recovery) are kept current; the
scenario inventory and automation backlog are a snapshot as of the date above — check
`notes/user-journeys.md` + `ls e2e/specs/` for today's coverage. Maps **every** interactive surface, state
machine, edge state, and chaos sequence across the tabs (5 at snapshot time; Doctor landed after — its journeys are tracked in `notes/user-journeys.md` and gated by `doctor.spec.ts` + `doctor-apply.online.spec.ts`), consolidates them into a runnable
scenario set, and tags each scenario with a regression-automation recommendation.

Sources: a full read of `src/views/**`, `src/App.tsx`, `src/ui/{DeviceStatus,ErrorBoundary}`,
`src/lib/{connectError,gates,firmware}`, the existing `e2e/specs/**` + `src/__tests__/**` +
`src-tauri/src/*` tests. Corrections to CLAUDE.md found during discovery are in §8.

---

## 0. How to run

`scripts/e2e.sh` is the turn-key runner (also `bun run e2e`):

- **`bash scripts/e2e.sh`** — OFFLINE, all specs against the SimDevice (fast, ~1.5 min, no hardware).
- **`bash scripts/e2e.sh online`** — ONLINE against the real unit (doctor → level → songs → copy, one
  at a time, ~25 min on the reference unit; varies by run). Preconditions: the unit plugged in
  and **Pro Control closed** (it holds the exclusive HID seize). The script pre-flights the
  handshake, runs each spec in its own invocation, and recovers the unit on every handled exit
  (`trap cleanup EXIT INT TERM` — so Ctrl-C and `kill` included): reamp-off + recall 001, plus a
  guarded clear of the scenario slots (`SCRATCH_SLOTS`, 400–410) only with `TMP_E2E_CLEAR_SCENARIO=1`.
  A SIGKILL or host crash skips the traps — recover manually with `cargo run --bin probe -- --reamp-off`.
- **`bash scripts/e2e.sh online auto`** — ONLINE, exactly the specs this diff requires
  (`scripts/gates.sh --required-online`, from `scripts/gate-map.tsv` and `scripts/gate-policy.tsv`);
  a diff that requires none exits 0 without touching the unit.
- **`bash scripts/e2e.sh online release`** — the same, over everything merged since the last release tag.
- **`bash scripts/pre-release.sh`** — the release-time run: prints every merged change since the last
  release tag that touched device-facing code (`scripts/gates.sh --device-changes`), then runs
  `online release` (`pre-release.sh all` runs the full set). PRs run the offline gates only; this is
  where the online suite runs, once per release, with the unit plugged in by hand.

Add a spec name — `copy` / `songs` / `doctor.online` / `level.online` — to run a single spec (e.g. `bash scripts/e2e.sh online level.online`).
Any passing online run merges the specs it ran into the tree's online stamp (one `#run` line per run, with
`os=`, `fw=` and `unit=`), so a failed spec re-runs alone. Soak never stamps, a level.online run stamps only
when the ffmpeg validation passed, and no run stamps unless the unit presents its stock USB interfaces.
Per-spec wall times land in `$TMPDIR/tmp-companion-e2e/online-timings.tsv` — date, spec, wall s, verdict, tree key,
then the spec's HARDWARE WAITS: settle s and capture s, read from the server's `GET /timing` (`rig::WAITS`) around
the spec, so the remainder of the wall time is runtime. Offline, `scripts/e2e-budgets.mjs` prints and persists
(`e2e/test-results/e2e-phases.json`) the build / run-wall / summed-test-time split; settles collapse offline.

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
| Copy                 | P1–P10               | M-P1–M-P5              | e2e + vitest happy paths                          | physical unplug mid-save (partial failure + Back gated in Vitest)   |
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
  FirmwareGate full-page ("Use it anyway"); Catalog/Settings stay reachable;
  DeviceStatus shows persistent "untested ⚠". "Use it anyway" unlocks per-mount.
- **C6 — Hot-plug detach.** While connected on Level, unplug → DeviceStatus→hollow, body→EmptyState,
  firmware/graph cleared, retry loop stays armed. Replug → auto-reconnects, body repopulates.
- **C7 — Tab routing.** Click through all 6 tabs (incl. Doctor, post-snapshot); each body wrapped in `ErrorBoundary key={tab}` —
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
  or "no change" / error); ProgressBar; Done medallion + Pro-Control restore note → resets to Step 1 when every target saved; a partial or total failure shows a warn medallion and returns to Step 2 with the unsaved edits (#291).
- **P10 — Multi-preset save** — two targets, different edits each; both written; re-open shows edits
  carried via optimistic cache patch (no second 22 s scan; `read_library_via_backup` called once).

### Monkey/chaos

- **M-P1 — Back keeps staged edits.** Edit blocks in Step 2 → Back → "Place the blocks" again →
  the edits and their undo steps are still there for every target still selected; a newly picked
  target starts fresh, a deselected one drops out (`retargetHistory`). Fixed for #292; gated by
  `CopyStagedEdits.test.tsx`.
- **M-P2 — Save-abort / disconnect mid-save.** Pull device during a 2-preset save → partial results.
  The done screen derives all / partial / none from the per-preset results (no green success unless
  every target saved); the cache is patched only for the presets that saved, including those the
  channel reported before a rejection; Done keeps the unsaved presets' edits on Step 2 (#291,
  `CopyStagedEdits.test.tsx`). The physical unplug is still untested.
- **M-P3 — Partial save failure** — one of N presets returns `error` → row shows warn-tri, others
  "updated", only updated items patch the cache. Gated by `CopyStagedEdits.test.tsx` (#291).
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

- **M-S1 — Rapid CRUD spam** — fire create/delete/rename back-to-back; the op queue must
  serialize (single in-flight, nothing dropped), buttons disable; no concurrent USB connect collision.
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

1. ✅ resolved (2026-07-05): **No "Disclaimer/Backup" wizard stage.** CLAUDE.md and
   `overlays/StepRail.tsx` (then in `WizardShell.tsx`) now both describe the 3-step rail (Set up · Level · Summary) —
   the doc/code mismatch is gone.
2. ✅ resolved (2026-07-05): **Row click does not recall the preset.** `LevelView.tsx` and
   `PresetRow.tsx` comments now agree with the code (click only selects; recall is owned by
   Pro Control / the footswitches).
3. ⚠️ still open (2026-07-05): **Catalog Effects disclosure has no collapse path** — `fxOpen` is
   only ever set `true`; once opened, the subcategory rows can't be hidden again. Minor, but a real
   one-way toggle. Confirmed via grep (`src/views/CatalogView.tsx`): `setFxOpen` is called exactly
   once, with `true` — the behavior described is unchanged.
4. ✅ resolved (2026-10-12, #292): **Copy "Back" no longer discards staged edits.** Re-entering
   Step 2 re-targets the history (`retargetHistory` in `copyModel.ts`): targets still selected keep
   their edits and undo steps, and only new targets start from their library graph.

---

## 9. Consolidation → regression-automation backlog

Mapping the scenarios above to the cheapest durable layer. "Have" = already covered.

| Layer                                                                                   | Add these (highest value first)                                                                                                                                                                                                                                                                                                                                     |
| --------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Rust unit** (have: proto/session/leveller/lufs/monitor/library/variants)              | `bpm_warning` propagation path; a `copy_apply` partial-failure (one slot `error`) shaping the streamed items.                                                                                                                                                                                                                                                       |
| **Vitest component** (have: most happy paths)                                           | **Cancel-mid-run RunBody → Summary "Not leveled" + levels-kept** (inject cancel); **partial-failure item** (`status:"error"`) in Level streaming (Copy: done, #291); **double-click guard** on Level/Copy/Songs primaries; BPM-warn Toast render; Copy Back keeps edits (done, #292); Settings calibration phase machine (countdown→recording→abort, `abortedRef`). |
| **Playwright offline** (have: copy/level/songs happy paths + `CatalogView` interaction) | **Add-song-to-setlist + reorder + remove** (current songs spec only makes an empty setlist); empty-library Copy/Level UX; backup-scan-failure rendering; tab-switch-during-scan. Settings spec (none exists); a Catalog Playwright spec is optional (the vitest one covers filters/sort/facets).                                                                    |
| **Playwright online** (have: all 3 specs GREEN online, attended)                        | The genuinely hardware-only ones: **Stop-mid-run leaves unit un-muted**; clamp/off-branch on a real hard target; cold-boot SetReport window; Pro-Control-grab red banner; ~22 s backup latency. Most overlap with the manual run order in §10.                                                                                                                      |
| **Manual / Claude-driven only** (cannot reasonably automate)                            | Real LUFS convergence accuracy; native-window chrome (Dock icon, focus, OS dialogs); `cliclick` left-display caveat; physical replug timing.                                                                                                                                                                                                                        |

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

## Recording: online run evidence and device traces

Every online and soak run keeps its evidence instead of overwriting the last run's, and every process that opens the device can record the wire. The recorder is `src-tauri/src/trace/` (its `//!` headers are the authority); the run folders are `scripts/e2e.sh`'s.

### Run folders

| Path                                                                         | Holds                                                                                                                                                                                                                                                                                                                 |
| ---------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `<root>`                                                                     | `$(git rev-parse --git-common-dir)/tmp-evidence`, made absolute. Inside `.git`, so it is never tracked, never hashed into a gate stamp, shared by every worktree and kept when a worktree is removed. `TMP_E2E_EVIDENCE_DIR` overrides it; an override inside the worktree must be gitignored, or the run refuses it. |
| `<root>/runs/<yyyymmddThhmmssZ>-<mode>-<sha7>[-dirty]/`                      | One run.                                                                                                                                                                                                                                                                                                              |
| `…/MANIFEST.json`                                                            | Run metadata: mode, specs, start and end, exit code, whether it validated and stamped, the tree key before and after, git sha, OS, ffmpeg, device firmware, `unit_config` (`stock` or `non-stock`), Scene Change Behavior, fixture stamp, the traces (file, bytes, device sessions, drops) and the WAV policy.        |
| `…/e2e_server.log`, `seed.log`, `online-spec-<spec>.log`, `soak-run-<n>.log` | The lane's logs, one set per run. `$LOG_DIR/e2e_server.log` stays a symlink to the current server log.                                                                                                                                                                                                                |
| `…/level-validate-expectations.jsonl`, `…/level-validate-wavs/`              | External-validation rows and their WAVs.                                                                                                                                                                                                                                                                              |
| `…/doctor-validate.jsonl`                                                    | Doctor validation rows.                                                                                                                                                                                                                                                                                               |
| `…/trace/<process>-<pid>.tct.jsonl`                                          | One trace per process that opened the device (the seed probe, the e2e server).                                                                                                                                                                                                                                        |
| `…/wav/<cap_id>.wav`                                                         | Each capture's processed pair (`TMP_TRACE_WAV=pair`).                                                                                                                                                                                                                                                                 |
| `…/field8/<list_index>.json`                                                 | The canonical field-8 body of each fixture slot the seed read.                                                                                                                                                                                                                                                        |
| `<root>/LAST_DEVICE_FW`                                                      | The firmware a device session last reported.                                                                                                                                                                                                                                                                          |
| `<root>/latest`                                                              | Symlink to the newest run.                                                                                                                                                                                                                                                                                            |

Everything except WAVs is kept. WAVs are kept for the newest `TMP_E2E_EVIDENCE_WAV_RUNS` runs (default 10). A run stopped with SIGINT still gets its MANIFEST, after the device recovery.

### Arming the recorder

Nothing records unless `TMP_TRACE_DIR` is set: no thread, no allocation, no file. The lane sets these for the seed probe and the e2e server; any probe or script arms it the same way.

| Variable                                                      | Meaning                                                           |
| ------------------------------------------------------------- | ----------------------------------------------------------------- |
| `TMP_TRACE_DIR`                                               | The run's `trace/` directory. `wav/` and `field8/` go next to it. |
| `TMP_TRACE_PROCESS`                                           | Process name in the trace file name (default: the executable's).  |
| `TMP_TRACE_RUN_ID`, `TMP_TRACE_GIT_SHA`, `TMP_TRACE_TREE_KEY` | Copied into the header.                                           |
| `TMP_E2E_UNIT_CONFIG`                                         | `stock` or `non-stock`, copied into the header.                   |
| `TMP_TRACE_WAV=pair`                                          | Write each capture's processed pair as a WAV.                     |
| `TMP_TRACE_FW_FILE`                                           | Where a device session's firmware version is written.             |

### TCT v1

JSON Lines, header first. A cut last line is legal (the server is killed, not stopped) and readers drop it. Every line starts with the same five fields, in this order, and each event's own fields follow in a fixed order, so plain `grep` works:

- `seq`: per process, gap-free.
- `sid`: one per opened transport (`null` when no transport is open).
- `t_us`: monotonic microseconds since the recorder started.
- `t_nom_ms`: the nominal clock, the sum of pump windows, nominal settles and nominal capture lengths up to this event.
- `ev`: the event type.

| `ev`      | Fields                                                                                                                                                                                                      | Notes                                                                                                                                                                                                                                                                                                                       |
| --------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `header`  | `tct` (1), `recorder`, `process`, `pid`, `os`, `wall_anchor_utc`, `run_id`, `git_sha`, `tree_key`, `fixture_stamp`, `unit_config`                                                                           | `wall_anchor_utc` is the wall time at `t_us` 0.                                                                                                                                                                                                                                                                             |
| `open`    | `backend`, `ok`, `err`, `ms`                                                                                                                                                                                | `backend` is `device` when the real device was opened and `sim` when the e2e factory's fake was; it comes from the branch that ran, never from the environment. A failed open is recorded too.                                                                                                                              |
| `out`     | `call`, `pump_ms`, `body`, `k`                                                                                                                                                                              | `call` is `send`, `transact`, `transact_chunked`, `pump` or `transact_eager`. `body` is the full hex body (`null` for `pump`). `k` is `<tms>.<inner>`, the first two field numbers.                                                                                                                                         |
| `in`      | `of`, `t_end_us`, `reports`, `k`, `err`                                                                                                                                                                     | `of` is the `seq` of the matching `out`; `t_us` is when the call started. `reports` are the raw input reports in hex; each `k` is the kind on a single frame or stream start and `cont` on a continuation.                                                                                                                  |
| `close`   |                                                                                                                                                                                                             | The transport was dropped: a connection boundary.                                                                                                                                                                                                                                                                           |
| `settle`  | `nominal_ms`, `applied_ms`                                                                                                                                                                                  | From `device_gate::settle_ms`. Raw `thread::sleep` sites are not counted.                                                                                                                                                                                                                                                   |
| `fw`      | `fw`                                                                                                                                                                                                        | Parsed from a `currentFwResponse` report.                                                                                                                                                                                                                                                                                   |
| `capture` | `cap_id`, `backend`, `t_start_us`, `t_end_us`, `stim_sha256`, `stim_frames`, `rate`, `tail_ms`, `channels`, `frames`, `lufs_i`, `true_peak_dbtp`, `spread_lu`, `peak`, `rms`, `engage`, `ctx`, `wav`, `err` | Loudness is the processed pair's; `peak` and `rms` cover every returned channel. `engage` is the last re-amp toggle seen on the wire, its `seq` and whether an echo arrived: context, not proof (a finite loudness is the proof). `ctx` names the sound (`label`, `slot`, `scene_slot`, `switch`) when the caller knows it. |
| `cmd`     | `name`, `args_sha256`, `ok`, `ms`                                                                                                                                                                           | From the e2e server's `/invoke` route only.                                                                                                                                                                                                                                                                                 |
| `state`   | `kind` (`field8`), `slot`, `bytes`, `complete`, `canon_sha256`, `pristine`, `file`                                                                                                                          | `pristine` is `ok`, the reason the body is not pristine, or `null` when not checked. A body with `complete: false` did not parse and is never the stored truth.                                                                                                                                                             |
| `drop`    | `what`, `count`                                                                                                                                                                                             | The recorder shed work (`wire`, `capture` or `other`). A trace with any `drop` is not a clean conformance source.                                                                                                                                                                                                           |

Per-report arrival times are not recorded; reports arrive at one per 2.143 ms (fw 1.8.58), which is enough to estimate them. `LiveReamp` and `reamp_measure` captures are not tapped.

**Canonical field-8 bodies** (`trace::canonical_json`, shared with any replay normalizer): keys sorted at every depth, arrays in order, every non-integer number through `f32` and then six significant digits (the device's own re-rounding moves a value by a few `f32` steps: an imported `0.49` reads back as `0.489999920129776`), integers and strings, node ids included, verbatim.

**Timing.** The caller's thread only clones bytes and hands them to a bounded queue; one writer thread per process does the hex and file work and flushes at least every 250 ms, and a second thread does each capture's hashing, metering and WAV, so a capture line lands when that finishes (with its own `t_us` and `t_nom_ms`) and nothing waits behind it. A transport drop waits at most 50 ms for the writer to catch up on the cheap lines, so a short-lived probe still leaves a complete trace and a mid-session reopen is not held up. A full queue drops and counts, never blocks, so the recorder cannot move an OUT write toward the 750 ms lapse.

### Privacy

Traces carry preset names and bodies from the unit they were recorded on, and captures are the device's DSP output. Raw evidence never leaves that machine: never commit it, attach it to an issue or PR, or upload it as a CI artifact. Only derived, redacted data is ever committed.

## Decision log (2026-10-11)

Decisions taken for making the offline e2e simulator match the device. Later entries amend earlier ones; none is rewritten in place.

- **Delivery.** Per-diff online spec selection and the side-by-side lanes land first, with fixes so tests run as root, an injectable test browser and a manual-only release trigger. Each later PR budgets two reviews and two online runs.
- **`/sim` routes.** The `/sim/*` handlers stay in `e2e_server.rs`.
- **Event bridge.** The UI event and Channel bridge replaces the stub. If Channels cannot be captured, it carries events only, and the gap is recorded in COVERAGE.md.
- **Attended tooling checks.** Two unstamped attended device sessions verify the checks that need the unit at intermediate trees.
- **Showcase firmware.** `showcase.json` reports firmware 1.8.58.
- **Runner.** No self-hosted runner is registered on this public repository. `device-gate` becomes a required status once the job that posts it lands. A hosted job that reads only the base branch posts it when no device check is needed; otherwise the maintainer posts it after the required lanes pass locally. `smoke` means `smoke:darwin`.
- **Releases.** Releases are batched and started manually. Once `device-gate` is required, a batch with device-facing changes publishes only after the maintainer posts a `device-release` status from a full local device run on that commit.
- **Reference unit.** Device evidence that stamps, records or calibrates comes only from a unit presenting its stock USB interfaces. The lane checks this and does not stamp otherwise. No second unit is planned.
- **Licence review.** Firmware analysis beyond a local first check needs the maintainer's licence review first. As of 2026-10-11 the review is not done and the local check has not run; nothing beyond it is approved.
- **Derived data.** The CPU table (`model-cpu.json`) and the Empty template's observable fields are public. The message table and conformance cases stay local until a follow-up decision, and would then be published only under allow-list redaction. No descriptor files, factory preset bodies, default parameters, IRs or user content are committed.
- **Real backups.** No real device backup is committed. A private check compares the simulator's archive with a real one locally.
