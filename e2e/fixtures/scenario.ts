import { expect, type Page } from "@playwright/test";

// Shared scenario setup for the dual-mode specs. The working presets live at slots
// 400-410 (the high scratch zone, clear of the user's real presets) and are the SAME
// fixed presets in both modes (deterministic — same blocks every run, validated against).
// OFFLINE they are baked into the backup fixture + the startup snapshot, so `ensureScenario`
// finds them and skips. ONLINE they start empty, so `ensureScenario` imports the identical
// committed presetJsons (`e2e_seed_scenario` → `scenario-presets.json`). `clearScenario`
// returns the unit to net-zero.
// This worker's own bridge (per-worktree base + parallel index) — see fixtures/port.ts.
import { SERVER } from "./port";

export interface Preset {
  name: string;
  slot: number;
}

/** A leveling candidate block, as `list_level_blocks` reports it — shared by every leveling
 *  spec that reads a block back directly (the online Friedman/Plumes arcs, the Preset24
 *  lazy-commit gap test). */
export interface LevelBlock {
  group_id: string;
  node_id: string;
  parameter_id: string;
  value: number;
}

// Role-based names (not slot numbers): the device stores these at userSlot = listIndex + 1
// (401/402/.../411), so a slot-numbered name would read off-by-one in the backup view.
// WHICH USE CASE EACH FIXTURE CARRIES: e2e/fixtures/COVERAGE.md (the matrix), pinned by
// `fixture_gates` in src-tauri/src/lib.rs. In brief — Rig: scene overlays, footswitch classes
// and the two Doctor damage signatures; Pedalboard: scene-free, the Copy source, EXP + link
// groups + a second-bank switch; Edge: gtrSplit, 8 scenes, the baked 2.6 kHz EQ-ring Doctor
// oracle and the off-USB lane, plus (P4-C) a `shared_write_is_scene_local` Boost/Solo
// anatomy — bypassed in base, un-bypassed ONLY by scene 3 "Solo"; Parallel: both lane amps live (joint-k / rebalance);
// Hiwatt 3S: a VERBATIM device export (the scene-conformance oracle — do not edit);
// Preset24: the stale-load / saturated-pedal footswitch fixture (level-fs-preset24.spec.ts),
// amended for the Plumes leveling regression (presetLevel 0.27, Twin outputLevel 0.28, Rat
// base-ON at 0.62 — see COVERAGE.md and scenario-loudness.json's own "405" comment).
// P3 additions (ADDITIONS, not replacements — 404/405 stay untouched structurally by P3):
// Combined Level: the new-flow leveling fixture (FS-alone, scene-alone "BASE SCENE",
// scene-that-enables-an-FS, parallel Deluxe Reverb + Marshall Plexi, a post-cab compressor).
// Doctor Oracle: 14 mixed-shape footswitches, one per Doctor spectral check, all bypassed in
// base. Preset24 Min / Hiwatt Min: the smallest presets still reproducing each incident's own
// bug class. Friedman 3S (P4, slot 410): a 3-FULL-overlay-scene (Rhythm/Lead/Base Scene)
// TubeScreamer->MarshallPlexi->CabSimTMS chain — the catalog-verified stand-in for a Friedman
// HBE-class 3-scene preset (no such amp is cataloged; MarshallPlexi is the closest high-gain
// British-voiced substitute).
export const SCENARIO: Preset[] = [
  { slot: 400, name: "E2E Rig" },
  { slot: 401, name: "E2E Pedalboard" },
  { slot: 402, name: "E2E Edge" },
  { slot: 403, name: "E2E Parallel" },
  { slot: 404, name: "E2E Hiwatt 3S" },
  { slot: 405, name: "E2E Preset24" },
  { slot: 406, name: "E2E Combined Level" },
  { slot: 407, name: "E2E Doctor Oracle" },
  { slot: 408, name: "E2E Preset24 Min" },
  { slot: 409, name: "E2E Hiwatt Min" },
  { slot: 410, name: "E2E Friedman 3S" },
];

export async function invoke(
  page: Page,
  cmd: string,
  args: Record<string, unknown> = {},
  timeoutMs?: number,
): Promise<unknown> {
  const res = await page.request.post(`${SERVER}/invoke`, {
    data: { cmd, args },
    // Playwright's 30 s default stands for ordinary commands (a hang should fail
    // fast); only the seed/teardown callers pass a long timeout — their online
    // sweep + imports legitimately run minutes.
    timeout: timeoutMs,
  });
  const env = (await res.json()) as {
    ok: boolean;
    data?: unknown;
    error?: unknown;
  };
  if (!env.ok) throw new Error(`${cmd} failed: ${JSON.stringify(env.error)}`);
  return env.data;
}

export async function listPresets(page: Page): Promise<Preset[]> {
  return (await invoke(page, "list_presets")) as Preset[];
}

/** Ensure every scenario preset exists at its slot (400-409). Offline: baked into the
 *  fixture + snapshot, so a name check suffices (SimDevice state is disposable).
 *  ONLINE: always route through the ownership-verified seed — it verifies every
 *  occupied target by fixture CONTENT MARKER (not name; a user preset coincidentally
 *  named "E2E Pedalboard" fails the seed loudly instead of being blessed and later
 *  saved-over / cleared), imports only what's missing, and fast-no-ops when the
 *  server's verified-seed flag is armed (the runner's `e2e_mark_seeded` POST after its
 *  fresh-process seed, or a prior verified call this run — cleared by a STRUCTURAL
 *  spec save, see `e2e_server.rs`'s `note_structural_save`) — so per-spec calls don't
 *  re-pay the multi-second, lockout-prone in-process device verify.
 *
 *  Mode is read from the SERVER via `isOnline`, never `process.env.TMP_E2E_ONLINE` —
 *  the same trap `clearScenario` below already avoids (its own comment: "Ask the
 *  SERVER, never `process.env.TMP_E2E_ONLINE`"). `scripts/e2e.sh` sets that var ONLY
 *  on the server's `cargo run` invocation, so the Playwright process never inherits
 *  it — a `process.env` read here always took the offline branch online too, a
 *  presence-only check that a structurally mutilated preset trivially passes, so
 *  `e2e_seed_scenario` (and its re-verify) was never even invoked online
 *  (2026-08-01 incident, third and final link — see the registry in
 *  `notes/user-journeys.md`; `doctor-apply.online.spec.ts`'s own comment records the
 *  first occurrence of this class). */
export async function ensureScenario(page: Page): Promise<void> {
  if (!(await isOnline(page))) {
    const list = await listPresets(page);
    const bySlot = new Map(list.map((p) => [p.slot, p.name]));
    const present = SCENARIO.every((s) => bySlot.get(s.slot) === s.name);
    if (present) return;
  }
  // The seed sweeps strays + imports over minutes, so it gets a long request
  // timeout (ordinary commands keep the default).
  await invoke(page, "e2e_seed_scenario", {}, 240_000);
}

/** Open the Level tab on a fresh page and wait for the connected header. Dismisses the
 *  one-shot startup backup disclaimer when present (localStorage-gated — only the first
 *  load). Shared by every spec that opens Level cold (was a byte-identical local copy in
 *  level-defaults.spec.ts and level-setup.spec.ts). */
export async function openLevel(page: Page): Promise<void> {
  await page.goto("/");
  const disclaimer = page.getByRole("button", { name: /backed up/i });
  if (await disclaimer.isVisible().catch(() => false)) await disclaimer.click();
  await expect(page.getByText(/connected · \d+\.\d+/)).toBeVisible({
    timeout: 20_000,
  });
}

/** The Base row's stable identity, mirroring `setupRowHookKey`'s `baseKey(slot)`
 *  (`leveling.ts`) — the `Pick`'s `tid` for a Base row's target picker is
 *  `target:${baseRowKey(slot)}`, unique per preset regardless of how many other
 *  rows (scenes/footswitches) that preset also has selected. */
export const baseRowKey = (slot: number): string => `p${String(slot)}`;

/** Select ONLY a preset's Base row (never its scenes/footswitches) — expand the caret, then
 *  tick the "Base Preset" child row alone. Kept even though the target picker no longer
 *  collides across a preset's other rows (each row now has its own `target:<rowKey>`
 *  selector) — this still isolates a base-clamp test to exactly the one row it means to
 *  drive. */
export async function selectBaseOnly(page: Page, name: string): Promise<void> {
  const filter = page.getByPlaceholder(/Filter by name or slot/i);
  await filter.fill(name);
  await page
    .getByTitle(/Show Base/)
    .first()
    .click();
  await page.getByText("Base Preset", { exact: true }).click();
  await filter.fill("");
}

/** Design 1a auto-opens only the LOWEST-slot preset group in Set up (`useGroupOpen`,
 *  `SetupPage.tsx`'s `groups[0]`) — every other selected preset's row starts collapsed,
 *  so its sound rows (and their `data-pick="target:<rowKey>"` triggers) aren't in the DOM
 *  yet. Click the group header (`data-preset-group={slot}`, `PresetGroupRow.tsx`) to
 *  expand it before targeting a row inside. A no-op if the group is already open. */
export async function ensurePresetGroupOpen(
  page: Page,
  slot: number,
): Promise<void> {
  const trigger = page.locator(`[data-pick="target:${baseRowKey(slot)}"]`);
  if (!(await trigger.isVisible().catch(() => false))) {
    await page.locator(`[data-preset-group="${String(slot)}"]`).click();
  }
}

/** Pick a Base row's target by its option id ("Rhythm"/"Crunch"/"Lead"). Uses
 *  `data-pick-option="target:<rowKey>:<id>"` (`Pick.tsx`, keyed by `setupRowHookKey` —
 *  `leveling.ts`) rather than the option's TEXT — a text-based
 *  `getByText(label,{exact:true}).last()` proved unreliable once a SECOND preset's picker
 *  opens while an earlier preset is already bound to the same label: Playwright's own
 *  actionability wait ("visible, enabled, stable" all pass) still hung on click, retried
 *  hundreds of times, and eventually timed out with "<div></div> subtree intercepts pointer
 *  events" — `.last()`'s re-resolved match apparently isn't a stable target across retries.
 *  The attribute selector is unique per (row, option) pair by construction, so there is
 *  never more than one match to begin with. */
export async function pickBaseTarget(
  page: Page,
  slot: number,
  label: string,
): Promise<void> {
  const key = baseRowKey(slot);
  await page.locator(`[data-pick="target:${key}"]`).click();
  await page.locator(`[data-pick-option="target:${key}:${label}"]`).click();
}

/** Drive the Level wizard's Base flow end to end for one or more presets: select each
 *  preset's Base row, submit, pick and confirm each target label, submit, and wait for
 *  Done/Accept. */
export async function runBaseLevel(
  page: Page,
  targets: { preset: Preset; label: string }[],
): Promise<void> {
  await openLevel(page);
  for (const { preset } of targets) {
    await selectBaseOnly(page, preset.name);
  }
  const n = String(targets.length);
  await page
    .getByRole("button", { name: new RegExp(`Level ${n} preset`) })
    .click();
  await page.getByText(/I.ve backed up with Pro Control/i).click();
  for (const { preset, label } of targets) {
    await ensurePresetGroupOpen(page, preset.slot);
    await pickBaseTarget(page, preset.slot, label);
  }
  // The picks must actually BIND — assert each row's trigger now reads its target (guards
  // a silent display-vs-value no-op an always-solving fake re-amp would otherwise hide).
  for (const { preset, label } of targets) {
    await expect(
      page.locator(`[data-pick="target:${baseRowKey(preset.slot)}"]`),
    ).toContainText(label);
  }
  await page
    .getByRole("button", { name: new RegExp(`Start.*${n} sound`) })
    .click();
  await expect(
    page.getByRole("button", { name: /^(Done|Accept)$/ }),
  ).toBeVisible({
    timeout: 240_000,
  });
}

/** Open Doctor and select each preset in `presets` by name (filter → click "Select preset
 *  to check" → clear filter). Shared by every doctor spec's selection step (was a
 *  byte-identical loop in doctor.spec.ts and doctor.online.spec.ts). */
export async function selectPresetsForCheck(
  page: Page,
  presets: Preset[],
): Promise<void> {
  await page.getByRole("button", { name: "Doctor" }).click();
  const filter = page.getByPlaceholder(/Filter by name or slot/i);
  for (const p of presets) {
    await filter.fill(p.name);
    await page.getByTitle("Select preset to check").first().click();
  }
  await filter.fill("");
}

/** Click "Check N sounds" then "Run check on N sounds" — the two-step Doctor run trigger,
 *  always immediately after `selectPresetsForCheck`. */
export async function runDoctorCheck(page: Page): Promise<void> {
  await page.getByRole("button", { name: /Check \d+ sounds/ }).click();
  await page.getByRole("button", { name: /Run check on \d+ sounds/ }).click();
}

/** Best-effort invoke: swallow errors (offline lacks some commands; online a teardown
 *  partial-failure must not mask the test's own result). Long timeout — the online
 *  clears/sweeps can run minutes. */
const quiet = (
  page: Page,
  cmd: string,
  args?: Record<string, unknown>,
): Promise<void> =>
  invoke(page, cmd, args, 240_000).then(
    () => undefined,
    () => undefined,
  );

/** Best-effort re-amp disengage — the between-tests safety (a test aborted mid-capture
 *  must not leave the unit input-muted for the next one). No-op offline. */
export const reampOff = (page: Page): Promise<void> =>
  quiet(page, "e2e_reamp_off");

/** The process-global session::REAMP_*_COUNT engage/disengage counters off the bridge.
 *  Cumulative across the server process — capture a baseline at test start and diff it
 *  (see `expectReampBalanced`) so an earlier surplus OFF can't mask a later unpaired ON. */
// Real re-amp (measure + verify captures + save) runs well past the invoke helper's
// 30 s default, so every ONLINE leveling/measure invoke gets a long request timeout.
export const LEVEL_T = 280_000;

// A base-leveling `level_preset` job (snake_case wire shape) — shared by the online
// leveling specs so the job literal exists once.
export const baseLevelJob = (slot: number, target: number) => ({
  slot,
  target_lufs: target,
  save: true,
  topology_id: "guitar-humbucker",
  calibration_lufs: null,
  stimulus_path: null,
  profile_id: null,
  block_group_id: null,
  block_node_id: null,
  block_parameter_id: null,
  block_value: null,
});

export async function reampCounters(
  page: Page,
): Promise<{ on: number; off: number }> {
  const res = await page.request.get(`${SERVER}/reamp/counters`);
  return (await res.json()) as { on: number; off: number };
}

/** Standing re-amp-OFF safety gate (PR #81 class): THIS TEST must have disengaged re-amp at
 *  least as often as it engaged, checked BEFORE the spec's own reampOff teardown rescue —
 *  so a run that strands the unit re-amp-engaged (input-muted) fails HERE, not masked by the
 *  teardown. Asserts on the per-test DELTA vs `baseline` (grab it with `reampCounters` before
 *  the run) — the counters are cumulative, so a cross-test surplus OFF must not credit a later
 *  unpaired engage. `offDelta >= onDelta` is the invariant (each capture pairs engage+disengage
 *  and every leveling/doctor lane adds a guaranteed final OFF, so a balanced run is off > on). */
export async function expectReampBalanced(
  page: Page,
  baseline: { on: number; off: number },
): Promise<void> {
  const { on, off } = await reampCounters(page);
  const onDelta = on - baseline.on;
  const offDelta = off - baseline.off;
  expect(
    offDelta,
    `re-amp OFF delta (${String(offDelta)}) must be >= ON delta (${String(onDelta)}) this test — a shortfall means the run left the unit re-amp-engaged`,
  ).toBeGreaterThanOrEqual(onDelta);
}

/** The SimDevice's ordered event log (offline only — online returns []). Used by the
 *  offline events-equality oracle to prove two identical runs write the same sequence. */
export async function simEvents(page: Page): Promise<unknown[]> {
  const res = await page.request.get(`${SERVER}/sim/events`);
  return (await res.json()) as unknown[];
}

/** Arm slot `slot`'s NEXT offline capture to return silence once (POST /sim/fault) — the
 *  leveller's no-signal path. Offline only (no-op online, no fake installed). Used to
 *  inject a mid-run item failure (level-defaults.spec.ts). */
export async function armCaptureFault(page: Page, slot: number): Promise<void> {
  const res = await page.request.post(`${SERVER}/sim/fault`, {
    data: { slot },
  });
  expect(res.ok(), "POST /sim/fault").toBeTruthy();
}

/** Arm the offline fake's lazy-commit latency (POST /sim/commit-latency) — the bug→gate
 *  regression for the same-slot stale-load incident (`level-fs-preset24.spec.ts`'s second
 *  test). MUST be called AFTER the per-test `/sim/reset` (the `page` fixture's own
 *  beforeEach) — a fresh fake always re-arms latency back at 0. No-op online (no fake
 *  installed): `TMP_SIM_COMMIT_LATENCY_MS` is offline-only, matching the whole lazy-commit
 *  model it configures. */
export async function armCommitLatency(page: Page, ms: number): Promise<void> {
  const res = await page.request.post(`${SERVER}/sim/commit-latency`, {
    data: { ms },
  });
  expect(res.ok(), "POST /sim/commit-latency").toBeTruthy();
}

/** Whether the server drives the REAL device — read from /health, which is AUTHORITATIVE:
 *  the Playwright process does not inherit TMP_E2E_ONLINE, so a mode-split spec must ask the
 *  server, not process.env. */
export async function isOnline(page: Page): Promise<boolean> {
  const res = await page.request.get(`${SERVER}/health`);
  return ((await res.json()) as { online?: boolean }).online === true;
}

/** End-of-scenario teardown. ONLINE the fixtures stay RESIDENT in the scratch slots
 *  (400-409): the run-start pristine-checking seed self-repairs anything a run leveled,
 *  so clearing here only forces the next run to re-import everything (~2 min of device
 *  churn per run for nothing — adversarial-reviewed 2026-08-01). Set
 *  TMP_E2E_CLEAR_SCENARIO=1 (or run `probe --clear <slot> <name>` per slot) for the
 *  on-demand net-zero clean.
 *  Recovery always runs: stray sweep + recall 001 + re-amp OFF. Best-effort — the backend
 *  guard refuses any slot not holding the scenario name, so a real preset is never cleared.
 *
 *  OFFLINE this is a NO-OP, because it has nothing left to undo: the `page` fixture POSTs
 *  `/sim/reset` before EVERY test, and that rebuilds the whole SimDevice from scratch —
 *  presets, scenes, songs, setlists, the re-amp latch and any armed capture fault — then
 *  reinstalls the 400-409 snapshot. So isolation between tests comes from the reset, not
 *  from this teardown, and the 7 bridged commands here were pure cost. Two things that do
 *  survive the reset are deliberately unaffected: the cumulative re-amp counters (every
 *  spec baseline-DIFFS them via `expectReampBalanced`) and `SCENARIO_VERIFIED` (read only
 *  by `e2e_seed_scenario`, which offline is unreachable because `ensureScenario` above
 *  early-returns once the reset has put the presets back). */
export async function clearScenario(page: Page): Promise<void> {
  // Ask the SERVER, never `process.env.TMP_E2E_ONLINE` — `scripts/e2e.sh` sets that var
  // only on the `cargo run` server invocation, so the Playwright process does NOT inherit
  // it (same trap documented in level.spec.ts's merged idempotency test). An env check here would read
  // "offline" during an ONLINE run and skip the device recovery — no re-amp OFF, leaving
  // the unit input-muted.
  if (!(await isOnline(page))) return;
  const resident = process.env.TMP_E2E_CLEAR_SCENARIO !== "1";
  if (!resident) {
    for (const s of SCENARIO) {
      await quiet(page, "e2e_clear_preset", {
        slot: s.slot,
        expectName: s.name,
      });
    }
  }
  // Sweep any stray scenario imports an aborted seed stranded in the user's bank
  // (imports land at the FIRST EMPTY slot anywhere; guarded per slot, fail-closed).
  await quiet(page, "e2e_clear_strays");
  await quiet(page, "e2e_load_preset", { slot: 0 }); // recall preset 001 — leave a known preset
  // Disengage re-amp so a Level run killed mid-capture can't leave the unit input-muted (the
  // latch is device-side; the command is a no-op offline).
  await quiet(page, "e2e_reamp_off");
}

/** The non-empty values of `attr` on every matching element inside one target's Copy card,
 *  in DOM (signal) order — read at runtime so a spec never hard-codes a unit's block names. */
function cardAttrValues(
  page: Page,
  cardName: string,
  attr: string,
): Promise<string[]> {
  return page
    .locator(`[data-target-card="${cardName}"] [${attr}]`)
    .evaluateAll(
      (els, a) => els.map((e) => e.getAttribute(a) ?? "").filter(Boolean),
      attr,
    );
}

/** The block labels rendered in one target's Copy card (the `data-block-tile` values). */
export const tileLabels = (page: Page, cardName: string): Promise<string[]> =>
  cardAttrValues(page, cardName, "data-block-tile");

/** The candidate block labels offered in an OPEN BlockEditor (the reference preset's
 *  distinct blocks) — for same/different-model picks. */
export const candidateLabels = (
  page: Page,
  cardName: string,
): Promise<string[]> => cardAttrValues(page, cardName, "data-candidate");
