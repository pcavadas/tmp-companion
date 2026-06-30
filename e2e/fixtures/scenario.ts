import type { Page } from "@playwright/test";

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
): Promise<unknown> {
  const res = await page.request.post(`${SERVER}/invoke`, { data: { cmd, args } });
  const env = (await res.json()) as { ok: boolean; data?: unknown; error?: unknown };
  if (!env.ok) throw new Error(`${cmd} failed: ${JSON.stringify(env.error)}`);
  return env.data;
}

export async function listPresets(page: Page): Promise<Preset[]> {
  return (await invoke(page, "list_presets")) as Preset[];
}

/** Ensure the three scenario presets exist at 400/401/402. Idempotent: present (offline
 *  fixture) → no-op; absent (online) → import the SAME committed presetJsons. */
export async function ensureScenario(page: Page): Promise<void> {
  const list = await listPresets(page);
  const bySlot = new Map(list.map((p) => [p.slot, p.name]));
  const present = SCENARIO.every((s) => bySlot.get(s.slot) === s.name);
  if (present) return; // offline: baked into the fixture + snapshot
  await invoke(page, "e2e_seed_scenario"); // online: import the deterministic presets
}

/** End-of-scenario teardown: clear any scenario slot we wrote (net-zero) and leave the unit
 *  on preset 001 (list index 0). Best-effort — the backend guard refuses any slot not holding
 *  the scenario name, so a real preset is never cleared. */
export async function clearScenario(page: Page): Promise<void> {
  // Every step is best-effort: offline the fixture resets per test (these commands aren't
  // present), and online a partial-failure must not mask the test's own result.
  const quiet = (cmd: string, args?: Record<string, unknown>): Promise<void> =>
    invoke(page, cmd, args).then(
      () => undefined,
      () => undefined,
    );
  for (const s of SCENARIO) {
    await quiet("e2e_clear_preset", { slot: s.slot, expectName: s.name });
  }
  await quiet("e2e_load_preset", { slot: 0 }); // recall preset 001 — leave a known preset
  // Disengage re-amp so a Level run killed mid-capture can't leave the unit input-muted (the
  // latch is device-side; the command is a no-op offline).
  await quiet("e2e_reamp_off");
}

/** The non-empty values of `attr` on every matching element inside one target's Copy card,
 *  in DOM (signal) order — read at runtime so a spec never hard-codes a unit's block names. */
function cardAttrValues(page: Page, cardName: string, attr: string): Promise<string[]> {
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
export const candidateLabels = (page: Page, cardName: string): Promise<string[]> =>
  cardAttrValues(page, cardName, "data-candidate");
