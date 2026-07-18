import { expect, type Page } from "@playwright/test";

// Shared scenario setup for the dual-mode specs. The three working presets live at slots
// 400/401/402 (the high scratch zone, clear of the user's real presets) and are the SAME
// fixed presets in both modes (deterministic — same blocks every run, validated against).
// OFFLINE they are baked into the backup fixture + the startup snapshot, so `ensureScenario`
// finds them and skips. ONLINE they start empty, so `ensureScenario` imports the identical
// committed presetJsons (`e2e_seed_scenario` → `scenario-presets.json`). `clearScenario`
// returns the unit to net-zero.
const SERVER = "http://127.0.0.1:7600";

export interface Preset {
  name: string;
  slot: number;
}

// Role-based names (not slot numbers): the device stores these at userSlot = listIndex + 1
// (401/402/403), so a slot-numbered name would read off-by-one in the backup view. The
// Reference is the Copy source; Target 1/2 are the edited presets.
export const SCENARIO: Preset[] = [
  { slot: 400, name: "E2E Reference" },
  { slot: 401, name: "E2E Target 1" },
  { slot: 402, name: "E2E Target 2" },
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

/** Ensure the three scenario presets exist at 400/401/402. Offline: baked into the
 *  fixture + snapshot, so a name check suffices (SimDevice state is disposable).
 *  ONLINE: always route through the ownership-verified seed — it verifies every
 *  occupied target by fixture CONTENT MARKER (not name; a user preset coincidentally
 *  named "E2E Target 1" fails the seed loudly instead of being blessed and later
 *  saved-over / cleared), imports only what's missing, and fast-no-ops when the
 *  server's verified-seed flag is armed (the runner's `e2e_mark_seeded` POST after its
 *  fresh-process seed, or a prior verified call this run) — so per-spec calls don't
 *  re-pay the multi-second, lockout-prone in-process device verify. */
export async function ensureScenario(page: Page): Promise<void> {
  if (!process.env.TMP_E2E_ONLINE) {
    const list = await listPresets(page);
    const bySlot = new Map(list.map((p) => [p.slot, p.name]));
    const present = SCENARIO.every((s) => bySlot.get(s.slot) === s.name);
    if (present) return;
  }
  // The seed sweeps strays + imports over minutes, so it gets a long request
  // timeout (ordinary commands keep the default).
  await invoke(page, "e2e_seed_scenario", {}, 240_000);
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

/** Whether the server drives the REAL device — read from /health, which is AUTHORITATIVE:
 *  the Playwright process does not inherit TMP_E2E_ONLINE, so a mode-split spec must ask the
 *  server, not process.env. */
export async function isOnline(page: Page): Promise<boolean> {
  const res = await page.request.get(`${SERVER}/health`);
  return ((await res.json()) as { online?: boolean }).online === true;
}

/** End-of-scenario teardown: clear any scenario slot we wrote (net-zero) and leave the unit
 *  on preset 001 (list index 0). Best-effort — the backend guard refuses any slot not holding
 *  the scenario name, so a real preset is never cleared. */
export async function clearScenario(page: Page): Promise<void> {
  for (const s of SCENARIO) {
    await quiet(page, "e2e_clear_preset", { slot: s.slot, expectName: s.name });
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
