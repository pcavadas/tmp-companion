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
 *  persisted-Twin read). */
export interface LevelBlock {
  group_id: string;
  node_id: string;
  parameter_id: string;
  value: number;
}

// Role-based names (not slot numbers): the device stores these at userSlot = listIndex + 1
// (401/402/.../411), so a slot-numbered name would read off-by-one in the backup view.
// WHICH USE CASE EACH FIXTURE CARRIES: e2e/fixtures/COVERAGE.md (the matrix), pinned by
// `src-tauri/src/fixture_gates.rs`. In brief — Rig: scene overlays, footswitch classes
// and the two Doctor damage signatures; Pedalboard: scene-free, the Copy source, EXP + link
// groups + a second-bank switch; Edge: gtrSplit, 8 scenes, the baked 2.6 kHz EQ-ring Doctor
// oracle and the off-USB lane, plus (P4-C) a `shared_write_is_scene_local` Boost/Solo
// anatomy — bypassed in base, un-bypassed ONLY by scene 3 "Solo"; Parallel: both lane amps live (joint-k / rebalance);
// Hiwatt 3S: a VERBATIM device export (the scene-conformance oracle — do not edit);
// Preset24: the stale-load / saturated-pedal footswitch fixture (e2e_server_tests.rs's base-boost gates; level.online.spec.ts's T5 online),
// amended for the Plumes leveling regression (presetLevel 0.27, Twin outputLevel 0.28, Rat
// base-ON at 0.62 — see COVERAGE.md and scenario-loudness.json's own "405" comment).
// P3 additions (ADDITIONS, not replacements — 404/405 stay untouched structurally by P3):
// Combined Level: the new-flow leveling fixture (FS-alone, scene-alone "BASE SCENE",
// scene-that-enables-an-FS, parallel Deluxe Reverb + Marshall Plexi, a post-cab compressor).
// Doctor Oracle: 13 mixed-shape footswitches, one per Doctor spectral check, all
// enabled-neutral in base. Preset24 Min / Hiwatt Min: the smallest presets still reproducing each incident's own
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

/** Ensure every scenario preset exists at its slot (400-410). Offline: baked into the
 *  fixture + snapshot, so a name check suffices (SimDevice state is disposable).
 *  ONLINE: always route through the ownership-verified seed — it verifies every
 *  occupied target by fixture CONTENT MARKER (not name; a user preset coincidentally
 *  named "E2E Pedalboard" fails the seed loudly instead of being blessed and later
 *  saved-over / cleared), imports only what's missing, and fast-no-ops when the
 *  server's verified-seed flag is armed (the runner's `e2e_mark_seeded` POST after its
 *  fresh-process seed, or a prior verified call this run — cleared by a STRUCTURAL
 *  spec save, see `e2e_server.rs`'s `note_structural_save`) — so per-spec calls don't
 *  re-pay the multi-second in-process device verify.
 *
 *  Mode is read from the SERVER via `isOnline`, never `process.env.TMP_E2E_ONLINE` —
 *  the same trap `clearScenario` below already avoids (its own comment: "Ask the
 *  SERVER, never `process.env.TMP_E2E_ONLINE`"). The Playwright process's env is no mode
 *  signal — a `process.env` read here always took the offline branch online too, a
 *  presence-only check that a structurally mutilated preset trivially passes, so
 *  `e2e_seed_scenario` (and its re-verify) was never even invoked online
 *  (2026-08-01 incident, third and final link — see the registry in
 *  `notes/user-journeys.md`, which also records the first occurrence of this class). */
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
  await expect(page.getByText(/Connected · firmware \d+\.\d+/)).toBeVisible({
    timeout: 20_000,
  });
}

/** The main sound row's stable identity, mirroring `setupRowHookKey`'s `baseKey(slot)`
 *  (`leveling.ts`) — the `Select`'s `tid` for that row's Target dropdown is
 *  `target:${baseRowKey(slot)}`, unique per preset regardless of how many other
 *  rows (scenes/footswitches) that preset also has selected. */
export const baseRowKey = (slot: number): string => `p${String(slot)}`;

/** Open a preset on the Level (or Doctor) list with its caret ("Open <name>"). The caret
 *  is there only once the backup scan has read the preset's scenes and footswitches, so
 *  this also waits for the scan. */
export async function openPreset(page: Page, name: string): Promise<void> {
  await page
    .getByRole("button", { name: `Open ${name}`, exact: true })
    .click({ timeout: 60_000 });
}

/** The list's checkbox for one sound of a preset ("Choose Rhythm in E2E Rig"). */
export const soundBox = (page: Page, sound: string, preset: string) =>
  page.getByRole("checkbox", {
    name: `Choose ${sound} in ${preset}`,
    exact: true,
  });

/** The list's checkbox for a whole preset ("Choose E2E Rig"). */
export const presetBox = (page: Page, preset: string) =>
  page.getByRole("checkbox", { name: `Choose ${preset}`, exact: true });

/** The Level list's main action once something is chosen: "Level N sounds" (it counts
 *  SOUNDS — the main sound, each scene, each footswitch). */
export const levelButton = (page: Page, sounds: number | "any") =>
  page.getByRole("button", {
    name:
      sounds === "any"
        ? /^Level \d+ sounds?$/
        : new RegExp(`^Level ${String(sounds)} sounds?$`),
  });

/** Select ONLY a preset's main sound (never its scenes/footswitches) — open it with the
 *  caret, then tick the "Main sound" sub-row alone. Kept even though each Set up row has
 *  its own `target:<rowKey>` dropdown — this still isolates a base-clamp test to exactly
 *  the one row it means to drive. */
export async function selectBaseOnly(page: Page, name: string): Promise<void> {
  const filter = page.getByPlaceholder(/Filter by name or slot/i);
  await filter.fill(name);
  await openPreset(page, name);
  await soundBox(page, "Main sound", name).click();
  await filter.fill("");
}

/** Pick a Set up dropdown's option by the e2e hooks `Select` carries:
 *  `data-select="<tid>"` on the trigger, `data-select-option="<tid>:<value>"` on each
 *  option (`ui/Select.tsx`; row tids are keyed by `setupRowHookKey`, `leveling.ts`). An
 *  attribute selector rather than the option's TEXT: two rows' lists can show the same
 *  words ("Lead"), and the attribute is unique per (dropdown, option) pair by
 *  construction, so there is never more than one match. */
export async function chooseOption(
  page: Page,
  tid: string,
  value: string,
): Promise<void> {
  await page.locator(`[data-select="${tid}"]`).click();
  await page.locator(`[data-select-option="${tid}:${value}"]`).click();
}

/** Pick a main sound row's target by name ("Rhythm"/"Crunch"/"Lead"). Every Set up row
 *  shows at once (no preset collapses), so no row needs opening first. */
export async function pickBaseTarget(
  page: Page,
  slot: number,
  label: string,
): Promise<void> {
  await chooseOption(page, `target:${baseRowKey(slot)}`, label);
}

/** Set up's one main action. It keeps its words whatever it waits for. */
export const startLeveling = (page: Page) =>
  page.getByRole("button", { name: "Start leveling", exact: true });

/** The Summary's main action — always "Done", whatever the outcome (everything is
 *  already saved). Its appearance is how a spec knows the run reached the Summary. */
export const summaryDone = (page: Page) =>
  page.getByRole("button", { name: "Done", exact: true });

/** The Summary's "Try again (N)", for any N or exactly `n`. */
export const tryAgainButton = (page: Page, n: number | "any" = "any") =>
  page.getByRole("button", {
    name: n === "any" ? /^Try again \(\d+\)$/ : `Try again (${String(n)})`,
    exact: true,
  });

/** Drive the Level flow's main-sound path end to end for one or more presets: select
 *  each preset's main sound, submit, pick and confirm each target, start, and wait for
 *  the Summary. */
export async function runBaseLevel(
  page: Page,
  targets: { preset: Preset; label: string }[],
): Promise<void> {
  await openLevel(page);
  for (const { preset } of targets) {
    await selectBaseOnly(page, preset.name);
  }
  await levelButton(page, targets.length).click();
  await page.getByText(/I.ve backed up with Pro Control/i).click();
  for (const { preset, label } of targets) {
    await pickBaseTarget(page, preset.slot, label);
  }
  // The picks must actually BIND — assert each row's dropdown now reads its target (guards
  // a silent display-vs-value no-op an always-solving fake re-amp would otherwise hide).
  for (const { preset, label } of targets) {
    await expect(
      page.locator(`[data-select="target:${baseRowKey(preset.slot)}"]`),
    ).toContainText(label);
  }
  await startLeveling(page).click();
  await expect(summaryDone(page)).toBeVisible({
    timeout: 240_000,
  });
}

/** Choose each preset in `presets` WHOLE on the Level tab by name (filter → its
 *  "Choose <name>" checkbox → clear filter) — the Level twin of `selectPresetsForCheck`. */
export async function selectPresetsForLevel(
  page: Page,
  presets: Preset[],
): Promise<void> {
  const filter = page.getByPlaceholder(/Filter by name or slot/i);
  for (const p of presets) {
    await filter.fill(p.name);
    await presetBox(page, p.name).click();
  }
  await filter.fill("");
}

/** Open Doctor and select each preset in `presets` by name (filter → its "Choose <name>"
 *  checkbox → clear filter). Shared by every doctor spec's selection step (was a
 *  byte-identical loop in doctor.spec.ts and doctor.online.spec.ts). */
export async function selectPresetsForCheck(
  page: Page,
  presets: Preset[],
): Promise<void> {
  // `exact`: the Level list's "E2E Doctor Oracle" name button also contains the word.
  await page.getByRole("button", { name: "Doctor", exact: true }).click();
  const filter = page.getByPlaceholder(/Filter by name or slot/i);
  for (const p of presets) {
    await filter.fill(p.name);
    await presetBox(page, p.name).click();
  }
  await filter.fill("");
}

/** Click "Check N sounds" — always immediately after `selectPresetsForCheck`. When Doctor
 *  asks which instrument plays the chosen presets (one has none saved yet), choose None
 *  for all of them (the instrument a check uses when none is chosen — the same stimulus
 *  as before Doctor asked) and click "Check N sounds" again. Doctor remembers the choice
 *  for each preset, so a later check of the same presets on the same server skips the
 *  question.
 *
 *  `expectInstrumentPage` makes the question required: use it for a check of presets
 *  this server has never been asked about. Every e2e_server starts each run from its own
 *  empty store (`e2e/playwright.config.ts`), so the page must show, and a store left over
 *  from an earlier run fails here instead of quietly skipping the page. */
export async function runDoctorCheck(
  page: Page,
  { expectInstrumentPage = false }: { expectInstrumentPage?: boolean } = {},
): Promise<void> {
  const check = page.getByRole("button", { name: /^Check \d+ sounds?$/ });
  await check.click();
  // Every page after the list carries Doctor's Steps; the Instrument page is the one
  // that asks.
  const ask = page.getByRole("heading", {
    name: "Which instrument plays these?",
  });
  if (expectInstrumentPage) {
    await expect(
      ask,
      "Doctor skipped its Instrument page: this server's store already holds an instrument for these presets",
    ).toBeVisible();
  } else {
    await expect(
      ask.or(page.getByRole("list", { name: "Steps" })),
    ).toBeVisible();
    if (!(await ask.isVisible())) return;
  }
  const forAll = page.locator('[data-select="inst-all"]');
  if (await forAll.isVisible()) {
    await forAll.click();
    await page.locator('[data-select-option="inst-all:none"]').click();
  } else {
    const one = page.locator('[data-select^="inst:"]').first();
    const tid = await one.getAttribute("data-select");
    await one.click();
    await page.locator(`[data-select-option="${tid ?? ""}:none"]`).click();
  }
  await check.click();
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

/** The device's re-amp state, read back from both stores on a fresh connection
 *  (`e2e_reamp_state` → `Session::read_reamp_state`). `null` = that store never answered.
 *  Offline the SimDevice answers from its latch. Opens the device, so call it after reading
 *  `/sim/events` in a test: its heartbeats add `Heartbeat` events. */
export async function reampState(
  page: Page,
): Promise<{ mixer: boolean | null; settings: boolean | null }> {
  return (await invoke(page, "e2e_reamp_state")) as {
    mixer: boolean | null;
    settings: boolean | null;
  };
}

/** Standing re-amp-OFF safety gate (PR #81 class), checked BEFORE the spec's own reampOff
 *  teardown rescue, so a run that strands the unit re-amp-engaged (input-muted) fails HERE.
 *  The oracle is the DEVICE: its Mixer store (the audio path) must read OFF, and the
 *  Settings flag too when it answers. A missing Mixer reply fails — never read silence as
 *  OFF. The per-test send-counter DELTA vs `baseline` (grab it with `reampCounters` before
 *  the run) stays as a second, diagnostic assertion: every capture pairs engage+disengage
 *  and every leveling/doctor lane adds a guaranteed final OFF, so `offDelta >= onDelta`. */
export async function expectReampBalanced(
  page: Page,
  baseline: { on: number; off: number },
): Promise<void> {
  const { on, off } = await reampCounters(page);
  const onDelta = on - baseline.on;
  const offDelta = off - baseline.off;
  const counts = `(re-amp sends this test: ON ${String(onDelta)}, OFF ${String(offDelta)})`;
  const state = await reampState(page);
  expect(
    state.mixer,
    `the device's Mixer store gave no re-amp reply ${counts} — a missing reply is not OFF`,
  ).not.toBeNull();
  expect(
    state.mixer,
    `the device reports re-amp ON after the run ${counts} — the unit is left input-muted`,
  ).toBe(false);
  if (state.settings !== null) {
    expect(
      state.settings,
      `the device's Settings re-amp flag reads ON after the run ${counts}`,
    ).toBe(false);
  }
  expect(
    offDelta,
    `re-amp OFF delta (${String(offDelta)}) must be >= ON delta (${String(onDelta)}) this test — a shortfall means the run left the unit re-amp-engaged`,
  ).toBeGreaterThanOrEqual(onDelta);
}

/** The offline fake's state (`GET /sim/state`): re-amp latch, current slot and scene, the
 *  unanswered-request histogram and the kinds outside its census, the armed injections.
 *  Online: `{online: true}`. */
export async function simState(page: Page): Promise<Record<string, unknown>> {
  const res = await page.request.get(`${SERVER}/sim/state`);
  return (await res.json()) as Record<string, unknown>;
}

/** Arm one protocol fault on the offline fake (`POST /sim/inject`) and return its tag. The
 *  helper names below carry the tag, so a spec reads whether the fault can happen on the
 *  device. `/sim/reset` (every test) disarms them all. */
async function simInject(
  page: Page,
  kind: string,
  args: Record<string, unknown>,
  expected: string,
): Promise<void> {
  const res = await page.request.post(`${SERVER}/sim/inject`, {
    data: { kind, ...args },
  });
  const body = (await res.json()) as { ok: boolean; tag?: string };
  expect(body.ok, `POST /sim/inject ${kind}`).toBe(true);
  expect(body.tag, `${kind}'s reachability tag`).toBe(expected);
}

/** A fault fw 1.8.58 can produce: `lose-first-confirm`, `reject-replace` ({nth}),
 *  `drop-next-reamp-off` (a stale Settings flag makes the next OFF a no-op). */
export const simInjectReachable = (
  page: Page,
  kind: "lose-first-confirm" | "reject-replace" | "drop-next-reamp-off",
  args: Record<string, unknown> = {},
): Promise<void> => simInject(page, kind, args, "reachable");

/** A fault fw 1.8.58 cannot produce: `drop-first-structural`, `bare-confirms`. */
export const simInjectUnreachable = (
  page: Page,
  kind: "drop-first-structural" | "bare-confirms",
): Promise<void> => simInject(page, kind, {}, "unreachable");

/** Unverified on fw 1.8.58: `stale-push-after-save` (seen once on an older firmware),
 *  `reamp-off-never-lands` (no OFF clears the re-amp latch). */
export const simInjectUnverified = (
  page: Page,
  kind: "stale-push-after-save" | "reamp-off-never-lands",
): Promise<void> => simInject(page, kind, {}, "unverified");

/** Not the device's shape (a lapse cuts with no final frame): `truncated-scene-push`. */
export const simInjectNotDeviceShaped = (
  page: Page,
  args: { scene: number; cutBefore: string },
): Promise<void> =>
  simInject(page, "truncated-scene-push", args, "not-device-shaped");

/** The SimDevice's ordered event log (offline only — online returns []). Used by the
 *  offline events-equality oracle to prove two identical runs write the same sequence. */
export async function simEvents(page: Page): Promise<unknown[]> {
  const res = await page.request.get(`${SERVER}/sim/events`);
  return (await res.json()) as unknown[];
}

/** POST a `/sim/*` knob on the offline fake (each is a no-op online). */
async function simPost(
  page: Page,
  path: string,
  data: Record<string, number>,
): Promise<void> {
  const res = await page.request.post(`${SERVER}${path}`, { data });
  expect(res.ok(), `POST ${path}`).toBeTruthy();
}

/** Arm slot `slot`'s NEXT offline capture to return silence once (POST /sim/fault) — the
 *  leveller's no-signal path. Tagged approximate: a failed device inject reads the
 *  stationary output floor, not digital silence. Offline only (no-op online, no fake installed). Used to
 *  inject a mid-run item failure (level-defaults.spec.ts). */
export async function armCaptureFault(page: Page, slot: number): Promise<void> {
  await simPost(page, "/sim/fault", { slot });
}

/** Harness timing only. Hold every offline capture for `ms` (POST /sim/capture-delay), so a run is still in
 *  flight when a spec presses Stop. Call AFTER the per-test `/sim/reset`, which clears it. */
export async function armCaptureDelay(page: Page, ms: number): Promise<void> {
  await simPost(page, "/sim/capture-delay", { ms });
}

/** Wait until the run has engaged re-amp past `baseline` — i.e. a capture is in flight
 *  (held by `armCaptureDelay`), so a Stop pressed now lands MID-capture, not before the
 *  run command reached the device (where its own start-of-run reset would eat it). */
export async function waitForCaptureInFlight(
  page: Page,
  baseline: { on: number; off: number },
): Promise<void> {
  await expect
    .poll(async () => (await reampCounters(page)).on, { timeout: 30_000 })
    .toBeGreaterThan(baseline.on);
}

/** Offline teardown gate: the fake's device server never aborted this test. A request the
 *  real unit answers by aborting its server (a `replaceNode` naming a node the group lacks)
 *  shows on `/sim/state` as `server_aborts`; no product path may reach one. Online
 *  `/sim/state` is `{online: true}` and nothing is checked. */
export async function expectNoServerAborts(page: Page): Promise<void> {
  const state = await simState(page);
  if (state.online === true) return;
  expect(
    state.server_aborts,
    "the offline fake recorded a device-server abort (see /sim/events: ServerAbort)",
  ).toBe(0);
}

/** Whether the server drives a REAL device (the unit, or a remote device, which /health also flags
 *  as `remote`) — read from /health, which is AUTHORITATIVE; the Playwright process's env is
 *  no mode signal, so never read process.env. */
export async function isOnline(page: Page): Promise<boolean> {
  const res = await page.request.get(`${SERVER}/health`);
  return ((await res.json()) as { online?: boolean }).online === true;
}

/** End-of-scenario teardown. OFFLINE it asserts the fake's server never aborted
 *  (`expectNoServerAborts`). ONLINE the fixtures stay RESIDENT in the scratch slots
 *  (400-410): the run-start pristine-checking seed self-repairs anything a run leveled,
 *  so clearing here only forces the next run to re-import everything (~2 min of device
 *  churn per run for nothing — adversarial-reviewed 2026-08-01). Set
 *  TMP_E2E_CLEAR_SCENARIO=1 (or run `probe --clear <slot> <name>` per slot) for the
 *  on-demand net-zero clean.
 *  Recovery always runs: stray sweep + recall 001 + re-amp OFF. Best-effort — the backend
 *  guard refuses any slot not holding the scenario name, so a real preset is never cleared.
 *
 *  OFFLINE there is nothing left to undo beyond that check: the `page` fixture POSTs
 *  `/sim/reset` before EVERY test, and that rebuilds the whole SimDevice from scratch —
 *  presets, scenes, songs, setlists, the re-amp latch and any armed capture fault — then
 *  reinstalls the 400-410 snapshot. So isolation between tests comes from the reset, not
 *  from this teardown, and the 7 bridged commands here were pure cost. Two things that do
 *  survive the reset are deliberately unaffected: the cumulative re-amp counters (every
 *  spec baseline-DIFFS them via `expectReampBalanced`) and `SCENARIO_VERIFIED` (read only
 *  by `e2e_seed_scenario`, which offline is unreachable because `ensureScenario` above
 *  early-returns once the reset has put the presets back). */
export async function clearScenario(page: Page): Promise<void> {
  // Ask the SERVER, never `process.env.TMP_E2E_ONLINE` — the Playwright process's env is
  // no mode signal (same trap documented in level.spec.ts's merged idempotency test). An env check here would read
  // "offline" during an ONLINE run and skip the device recovery — no re-amp OFF, leaving
  // the unit input-muted.
  if (!(await isOnline(page))) {
    await expectNoServerAborts(page);
    return;
  }
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

/** The block models on Copy's source strip (the `data-block-tile` values in the "Copy
 *  from" card), in signal order — for same/different-model picks. */
export const sourceLabels = (page: Page): Promise<string[]> =>
  page
    .locator("[data-source-card] [data-block-tile]")
    .evaluateAll((els) =>
      els.map((e) => e.getAttribute("data-block-tile") ?? "").filter(Boolean),
    );
