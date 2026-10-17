// src/views/level/runEngine.ts — the leveling run ENGINE: plain async code, no React and
// no store.
//
// `runEngine(items, deps, emit)` drives a run by composing shipped commands per chosen row:
//   • BASE scene (or a scene-less "Whole preset") → level_preset (preset `presetLevel`,
//     preset-to-preset loudness).
//   • FS scenes → list_level_blocks (amp candidates, once per preset, cached) then
//     level_scenes_apply_batched with ADJACENT same-preset scenes sharing an instrument
//     batched into ONE call regardless of per-scene target (each job carries its own
//     targetLufs, like the footswitch lane) (amp `outputLevel`, scene-to-scene; per-scene
//     progress rides the channel) — per-scene calls re-loaded the preset each time,
//     flashing the unit back to base twice per scene.
//   • Block-acting footswitches → level_footswitches_apply, batched the same way.
// `planBatches` is the one pure grouping rule behind both batched lanes. Each step is
// isolated: a per-item failure becomes "skipped", never aborting the run, and anything
// uncaught marks every unresolved row skipped, so the run always ends readable. Every
// backend command ends its own run with re-amp OFF on a fresh connection (danger.md).
//
// The engine never mutates a RunItem: every change is an immutable `ItemPatch`, emitted in
// an `EngineStep` at the points the run table should repaint. `levelRunStore` applies the
// steps (`applyPatches`) to the state it publishes, so a published item is never touched
// again. The device commands arrive through `deps`, so tests drive the engine with fakes.

import type {
  levelFootswitchesApply,
  levelPreset,
  levelScenesApplyBatched,
  listLevelBlocks,
} from "../../lib/invoke";
import { errorReampStillOn, errorSkipKind } from "../../lib/connectError";
import { errMsg } from "../../lib/format";
import { MODELS } from "../../models/catalog";
import { resolveDeviceId } from "../../models/deviceId";
import {
  buildLevelJob,
  toFootswitchJobWire,
  type RunItem,
  type TargetLufsOf,
} from "./leveling";
import type {
  AmpCandidate,
  BatchLevelRun,
  ClampKind,
  LevelBlock,
  Profile,
  SkipKind,
  Verification,
} from "../../lib/types";

// AMP model ids (the catalog's amp categories) — the amp's outputLevel knob is the
// one with real loudness authority. A non-amp level control saturates to ~1.7 LU;
// preamp/master/volume params alter the preset's sound, so they're excluded.
const AMP_CATS = new Set([
  "Combo Amps",
  "Amp Heads",
  "Half Stacks",
  "Bass Amps",
]);
const bidsWhere = (keep: (m: (typeof MODELS)[number]) => boolean) =>
  new Set(
    MODELS.filter(keep)
      .map((m) => m.bid)
      .filter((bid): bid is string => bid != null),
  );
const AMP_BIDS = bidsWhere((m) => AMP_CATS.has(m.cat));

// Envelope-follower effects (`effect_type: "Envelope Filter"` — the guide's own
// discriminator, so manual CryBaby wahs, which are expression-driven, stay out).
// Their response tracks the stimulus's attack/decay envelope, so the synthetic clip
// can park them in a different regime than real playing — any preset carrying one
// gets the "envelope" verify-by-ear cause regardless of how clean the numbers look.
// Pedal fender_ids on the wire are exact (no CabIR-class suffixes) → plain Set.has.
const ENVELOPE_BIDS = bidsWhere((m) => m.et === "Envelope Filter");

// Live fallback only: filter a live block read down to amp `outputLevel` candidates
// (the common path uses the startup-backup candidates instead). A discovered amp
// block may carry merged cab/IR/convolution suffixes the catalog's bare amp bids
// lack (e.g. "ACD_HiwattDR103CanModCabIR") — `baseDeviceId` strips them via the
// canonical suffix set so amp+cab(+reverb) combo blocks still match their amp bid.
function ampBlocks(blocks: LevelBlock[]): AmpCandidate[] {
  return blocks
    .filter(
      (b) =>
        AMP_BIDS.has(resolveDeviceId(b.model_id, (id) => AMP_BIDS.has(id))) &&
        b.parameter_id === "outputLevel",
    )
    .map((b) => ({
      groupId: b.group_id,
      nodeId: b.node_id,
      parameterId: b.parameter_id,
      value: b.value,
    }));
}

// ── pure result mappers ─────────────────────────────────────────────────────────

/** The result fields the per-item mappers read — the STRUCTURAL subset shared by
 *  `LevelResult` (preset/scene) and `FootswitchLevelResult` (footswitch). The latter
 *  has no `verify_by_ear`, so its by-ear cause is `wet_floor` or none. */
export interface LevelOutcomeFields {
  clamped: boolean;
  clamp_reason: string | null;
  verify_lufs: number | null;
  predicted_lufs: number;
  verify_by_ear?: boolean;
  /** Footswitch rows only — `LevelResult` (preset/scene) has no such field, so it stays
   *  optional and those lanes simply never report an unconverged row. */
  unconverged?: boolean;
  /** Footswitch rows only: the clamp's pinned bound is the wet/mix floor, not headroom —
   *  see `FootswitchLevelResult.wet_floor`. */
  wet_floor?: boolean;
  /** Preset/scene rows only (footswitch rows always measure their result). */
  verification?: Verification;
  /** The clamp's cause (`ClampKind`); null when the row did not clamp. */
  clamp_kind?: ClampKind | null;
}

/** What a batched command's RETURNED rows add to its streamed ones: the batch save's
 *  verdict (`persist_mismatch`, read back after the save), which the channel items were sent
 *  too early to carry. Shared by `LevelResult` and `FootswitchLevelResult`. The run-end
 *  re-amp verdict is not read from the rows: it comes back beside them (`BatchLevelRun`). */
export interface RunEndFields {
  persist_mismatch: boolean | null;
}

// A `clamp_reason` is set ONLY when the leveled signal isn't effectively reaching the USB 1/2
// capture — a silent capture for preset/footswitch jobs (output not routed to USB), or the
// scene path's no-authority case (a big amp-outputLevel change doesn't move the capture:
// off-branch) → its own `offbranch` outcome ("not on USB 1/2"), which a re-level can't fix.
// A plain headroom/authority clamp (the knob has real effect but can't reach target) has
// `clamped` set with NO reason → "clamped at X". `unconverged` is last of the miss states
// (the backend's own precedence): it is only ever set when `clamped` is not, and it means
// the knob had room left, so a re-run helps. A row whose check capture FAILED is reported
// the same way: its number is only a prediction, and a re-run confirms it.
export const unverifiedOf = (r: LevelOutcomeFields): boolean =>
  r.verification === "verify_failed";
export const outcomeOf = (r: LevelOutcomeFields): RunItem["outcome"] =>
  r.clamp_reason != null
    ? "offbranch"
    : r.clamped
      ? "clamped"
      : r.unconverged || unverifiedOf(r)
        ? "unconverged"
        : "done";
export const valueOf = (r: LevelOutcomeFields): number =>
  r.verify_lufs ?? r.predicted_lufs;
// Resolve to a SINGLE by-ear cause. `wet_floor` only ever rides on a footswitch row, which
// never sets `verify_by_ear` (scene-only), so the two can't collide in practice. There is no
// dynamics-spread cause (#241): a wide-spread capture is not a measurement problem.
export const byEarCause = (r: LevelOutcomeFields): RunItem["verifyByEar"] =>
  r.wet_floor ? "wet_floor" : r.verify_by_ear ? "rebalance" : undefined;

// ── batching ────────────────────────────────────────────────────────────────────

/** A run row that levels via amp `outputLevel` in scene mode — not Base (`presetLevel`), not
 *  a block-acting footswitch. */
const isSceneItem = (it: RunItem): boolean =>
  !it.isBase && it.footswitch == null && it.sceneSlot != null;

/** One backend call's worth of rows: `work[start..end)`. A `preset` batch is always one
 *  row; `footswitch` and `scene` batches gather ADJACENT rows of one preset sharing an
 *  instrument (per-row targets ride the jobs array). */
export interface Batch {
  lane: "preset" | "footswitch" | "scene";
  start: number;
  end: number;
}

/** Split sorted run rows into the backend calls that level them. */
export function planBatches(work: readonly RunItem[]): Batch[] {
  const batches: Batch[] = [];
  for (let start = 0; start < work.length;) {
    const head = work[start];
    const lane: Batch["lane"] =
      head.isBase || (head.footswitch == null && head.sceneSlot == null)
        ? "preset"
        : head.footswitch != null
          ? "footswitch"
          : "scene";
    const joins = (w: RunItem) =>
      (lane === "footswitch" ? w.footswitch != null : isSceneItem(w)) &&
      w.slot === head.slot &&
      w.instId === head.instId;
    let end = start + 1;
    if (lane !== "preset") while (end < work.length && joins(work[end])) end++;
    batches.push({ lane, start, end });
    start = end;
  }
  return batches;
}

// ── patches ─────────────────────────────────────────────────────────────────────

/** An immutable change to the row at `index`. */
export interface ItemPatch {
  index: number;
  patch: Partial<RunItem>;
}

/** A repaint point: the patches since the last step, the run table's step counter, and
 *  the batch-wide caption (issue 6b: "Saving…" / "Verifying…", which no row's own status
 *  can show — null clears it, so it never outlives the window it describes).
 *  `resolvedKey` names a row that just reached its result (BUG-4: closing the wizard
 *  deselects it), and drops the live meter — the result row is the confirm. */
export interface EngineStep {
  patches: readonly ItemPatch[];
  currentIndex: number;
  tailMessage: string | null;
  resolvedKey: string | null;
}

/** Apply `patches` to `items`, copying every touched row and the array — never mutating. */
export function applyPatches(
  items: readonly RunItem[],
  patches: readonly ItemPatch[],
): RunItem[] {
  const next = [...items];
  for (const { index, patch } of patches)
    next[index] = { ...next[index], ...patch };
  return next;
}

// ── the engine ──────────────────────────────────────────────────────────────────

/** Everything the engine needs, captured when the run starts. */
export interface EngineDeps {
  levelPreset: typeof levelPreset;
  listLevelBlocks: typeof listLevelBlocks;
  levelScenesApplyBatched: typeof levelScenesApplyBatched;
  levelFootswitchesApply: typeof levelFootswitchesApply;
  /** Read live: the stop handler flips it while a command is in flight. */
  isCancelled: () => boolean;
  profileById: (id: string) => Profile | null;
  targetLufsByName: TargetLufsOf;
  /** Per-preset amp `outputLevel` candidates from the startup backup, keyed by 0-based
   *  list index — so a scene run never needs a live discovery round-trip. */
  ampCandidates: Map<number, AmpCandidate[]>;
  /** Per-preset block roster (fender_ids), keyed by 0-based list index. */
  blocksByIndex: Map<number, string[]>;
  /** The presets (0-based list indices) whose path-MERGE scenes get their two
   *  parallel-amp lanes equalized before joint-k (Set up's per-preset opt-in). */
  rebalance: ReadonlySet<number>;
  /** The main-sound rows an EARLIER run of this flow carried (the Summary's Try again):
   *  a scene batch whose preset's main sound is not in this run still anchors on it, with
   *  the same target, exactly as the first run's batch did. Empty on a first run. */
  earlierBaseRows?: readonly RunItem[];
}

/** A batch row the run levels: its item, its index in the run, and its target's loudness. */
interface Runnable {
  item: RunItem;
  index: number;
  targetLufs: number;
}

/** What a whole run says beyond its rows. */
export interface RunVerdict {
  /** The run-end re-amp verdict: `true` when any command of the run said re-amp may still
   *  be on; `false` when at least one command read it back off and none said otherwise;
   *  `null` when no command of the run read it back at all (every row was refused or
   *  skipped before reaching the unit, or each command failed without saying), so the
   *  run knows nothing new about the unit. */
  reampStillOn: boolean | null;
}

/** Drive `items` (already in `sortRunItems` order) one batch at a time. Resolves once
 *  every batch has ended or a stop was seen between batches, with the run's verdict; never
 *  rejects. */
export async function runEngine(
  items: readonly RunItem[],
  deps: EngineDeps,
  emit: (step: EngineStep) => void,
): Promise<RunVerdict> {
  const { isCancelled, profileById, targetLufsByName } = deps;
  let work: readonly RunItem[] = items;
  let pending: ItemPatch[] = [];
  let lastIndex = 0;
  // The run's re-amp verdict (`RunVerdict.reampStillOn`). A command that RETURNED always
  // read re-amp back (every leveling command ends with the guaranteed OFF), so its flag
  // is a real reading either way. A REJECTED command says only "still on": a rejection
  // without the flag may have been refused before anything engaged, so it reads nothing.
  let reampStillOn: boolean | null = null;
  const noteReamp = (stillOn: boolean, readBack: boolean) => {
    if (stillOn) reampStillOn = true;
    else if (readBack && reampStillOn === null) reampStillOn = false;
  };

  const patch = (index: number, p: Partial<RunItem>) => {
    const change = { index, patch: p };
    pending.push(change);
    work = applyPatches(work, [change]);
  };
  // `lastIndex` lets a tail-only progress item (no row of its own) re-publish the caption
  // without inventing a step index.
  const publish = (
    currentIndex: number,
    tailMessage: string | null = null,
    resolvedKey: string | null = null,
  ) => {
    lastIndex = currentIndex;
    const patches = pending;
    pending = [];
    emit({ patches, currentIndex, tailMessage, resolvedKey });
  };

  // INVARIANT (BUG 2): at most ONE row is ever "active". Several rows once all rendered
  // "leveling · <the same LUFS>" because nothing cleared a row OUT of "active" when the
  // channel (or the optimistic group pre-flip) named a NEW one; `RunPage` names the active
  // row in its now line and points the one live meter at its target. Every activation goes
  // through here, demoting any OTHER active row back to "queued" (never touching a resolved
  // "result" row).
  const setSoleActive = (target: number) => {
    work.forEach((w, j) => {
      if (j !== target && w.status === "active") patch(j, { status: "queued" });
    });
    patch(target, { status: "active" });
  };
  // Shared per-item resolve tail: mark result and publish one step past it.
  const finish = (index: number, p: Partial<RunItem>) => {
    patch(index, { ...p, status: "result" });
    publish(index + 1, null, work[index].key);
  };
  // Envelope-follower presets get the "envelope" cause over any result-derived one: the
  // effect tracks the stimulus envelope, so the measurement itself is suspect.
  const resultPatch = (
    r: LevelOutcomeFields,
    envelope: boolean,
  ): Partial<RunItem> => ({
    outcome: outcomeOf(r),
    unverified: unverifiedOf(r),
    value: valueOf(r),
    verifyByEar: envelope ? "envelope" : byEarCause(r),
    clampKind: r.clamp_kind ?? null,
  });
  // ONE per-row contract for a batched backend call (only the map key differs): an
  // `active` item → the sole active row (its caption is the backend's, for the prepass,
  // and the run page shows none); `done` → its outcome; `error` → skipped, with the item's
  // typed `skipKind` (its `message` is kept for the log); anything else (e.g. the backend's
  // cancelled sentinel) is ignored. A `tail` caption rides a batch-wide item with NO valid
  // row key, so it is handled BEFORE the lookup and re-published at the CURRENT step.
  const batchResolve =
    <K>(rows: Map<K, number>, envelope: boolean) =>
    (
      key: K,
      status: string,
      result: LevelOutcomeFields | null,
      message?: string | null,
      skipKind?: SkipKind | null,
      tail?: string | null,
    ) => {
      if (tail != null) publish(lastIndex, tail);
      const idx = rows.get(key);
      if (idx == null) return;
      if (status === "active") {
        setSoleActive(idx);
        publish(idx);
      } else if (status === "done" && result) {
        finish(idx, resultPatch(result, envelope));
      } else if (status === "error") {
        finish(idx, {
          outcome: "skipped",
          skipReason: message ?? null,
          skipKind: skipKind ?? "other",
        });
      }
    };
  // A batched call does ~8 s of un-streamed setup (the field-8 read, the preset load)
  // before its first `active` item, so flip the group's first row active up front — or
  // the run reads "queued" everywhere with the meter hidden. `start` is the batch's own
  // index, never re-derived from the map, so the step counter can't jump backward.
  const markGroupActive = <K>(rows: Map<K, number>, start: number) => {
    const indices = [...rows.values()];
    if (indices.length === 0) return;
    setSoleActive(indices[0]);
    publish(start);
  };
  // After the call: a stopped run keeps un-run rows queued (still selected for a
  // follow-up run) but reverts the optimistic active row; otherwise every
  // row the channel never resolved is skipped with the call's failure (if any) and its
  // typed kind.
  const sweepUnresolved = <K>(
    rows: Map<K, number>,
    failure: { reason: string | null; kind: SkipKind },
  ) => {
    const cancelled = isCancelled();
    for (const idx of rows.values()) {
      if (cancelled) {
        if (work[idx].status === "active") patch(idx, { status: "queued" });
      } else if (work[idx].status !== "result") {
        finish(idx, {
          skipReason: failure.reason,
          skipKind: failure.kind,
          outcome: "skipped",
        });
      }
    }
  };
  /** A batched command's RETURN: the batch save's verdict onto the row each returned row
   *  names (`keyOf` → the batch's row map), and the run-end re-amp verdict — which comes
   *  back BESIDE the rows, whatever their count — onto every row of the batch, the ones
   *  never reached included. The streamed items were sent before either was known. */
  const stampRunEnd = <K, R extends RunEndFields>(
    batch: Batch,
    rows: Map<K, number>,
    returned: BatchLevelRun<R>,
    keyOf: (r: R) => K,
  ) => {
    for (const r of returned.rows) {
      const idx = rows.get(keyOf(r));
      if (idx != null)
        patch(idx, { persistMismatch: r.persist_mismatch === true });
    }
    stampReamp(batch, {
      stillOn: returned.reampStillOn,
      warning: returned.reampWarning ?? null,
    });
  };
  /** Copy a command's run-end re-amp verdict onto every row of the batch: the typed flag
   *  the screen reads, and the text for the log. */
  const stampReamp = (
    batch: Batch,
    reamp: { stillOn: boolean; warning: string | null },
  ) => {
    if (!reamp.stillOn && !reamp.warning) return;
    for (let j = batch.start; j < batch.end; j++)
      patch(j, {
        ...(reamp.stillOn ? { reampStillOn: true } : {}),
        ...(reamp.warning ? { reampWarning: reamp.warning } : {}),
      });
  };
  /** A failed command's typed facts: why (`skipKind`) and whether re-amp may still be on. */
  const failureOf = (error: unknown) => ({
    reason: errMsg(error),
    kind: errorSkipKind(error),
    reampStillOn: errorReampStillOn(error),
  });
  /** Skip the batch's rows `rows` (its runnable ones: a row refused for its target keeps
   *  that reason). */
  const skipBatch = (rows: readonly Runnable[], p: Partial<RunItem>) => {
    for (const r of rows)
      finish(r.index, { skipKind: "other", ...p, outcome: "skipped" });
  };
  /** The batch's rows whose target resolves, each with its loudness. Every other row is
   *  refused here, before anything reaches the unit, with the typed `no_target`: there is
   *  no fallback loudness to level it at. */
  const runnableOf = (batch: Batch): Runnable[] => {
    const out: Runnable[] = [];
    for (let j = batch.start; j < batch.end; j++) {
      const lufs = targetLufsByName(work[j].targetName);
      if (lufs == null) {
        finish(j, { outcome: "skipped", skipKind: "no_target" });
      } else {
        patch(j, { targetLufs: lufs });
        out.push({ item: work[j], index: j, targetLufs: lufs });
      }
    }
    return out;
  };
  const candCache = new Map<number, AmpCandidate[]>();

  try {
    for (const batch of planBatches(items)) {
      if (isCancelled()) break;
      const runnable = runnableOf(batch);
      if (runnable.length === 0) continue;
      const i = runnable[0].index;
      const it = runnable[0].item;
      const profile = profileById(it.instId);
      const envelope = (deps.blocksByIndex.get(it.slot) ?? []).some((id) =>
        ENVELOPE_BIDS.has(id),
      );
      const wireProfile = {
        topologyId: profile?.topology_id ?? null,
        calibrationLufs: profile?.calibration_lufs ?? null,
        profileId: profile?.id ?? null,
      };

      if (batch.lane === "preset") {
        setSoleActive(i);
        publish(i);
        // A scene with no wire slot.
        let p: Partial<RunItem> = { outcome: "skipped", skipKind: "other" };
        try {
          if (it.isBase) {
            const res = await deps.levelPreset(
              buildLevelJob(
                it.slot,
                runnable[0].targetLufs,
                profile,
                true,
                it.baseHandle,
              ),
            );
            noteReamp(res.reamp_still_on, true);
            p = {
              ...resultPatch(res, envelope),
              truePeakDbtp: res.true_peak_dbtp,
              baseBoost: res.base_boost,
              persistMismatch: res.persist_mismatch === true,
              reampWarning: res.reamp_warning ?? null,
              reampStillOn: res.reamp_still_on,
            };
          }
        } catch (error) {
          // One sound's failure shouldn't abort the run — retain its reason, its typed
          // kind, and whether the run left re-amp on.
          const f = failureOf(error);
          noteReamp(f.reampStillOn, false);
          p = {
            skipReason: f.reason,
            skipKind: f.kind,
            outcome: "skipped",
            ...(f.reampStillOn ? { reampStillOn: true } : {}),
          };
        }
        finish(i, p);
        continue;
      }

      if (batch.lane === "footswitch") {
        // Block-acting FOOTSWITCHES: the backend measures every switch, then writes them
        // all on ONE session and saves the preset ONCE. Bake vs assign stays internal.
        const switches = runnable.flatMap((r) =>
          r.item.footswitch != null
            ? [{ ...r, footswitch: r.item.footswitch }]
            : [],
        );
        const bySwitch = new Map(
          switches.map((r) => [r.footswitch.switchIndex, r.index] as const),
        );
        const resolveFs = batchResolve(bySwitch, envelope);
        markGroupActive(bySwitch, i);
        let failure: ReturnType<typeof failureOf> | null = null;
        try {
          const run = await deps.levelFootswitchesApply(
            {
              slot: it.slot,
              jobs: switches.map((r) =>
                toFootswitchJobWire(r.footswitch, r.targetLufs),
              ),
              save: true,
              ...wireProfile,
            },
            (item) => {
              resolveFs(
                item.switch,
                item.status,
                item.result,
                item.message,
                item.skipKind,
              );
            },
          );
          // The rows streamed before the save and the run-end re-amp OFF; both verdicts
          // ride the return only.
          noteReamp(run.reampStillOn, true);
          stampRunEnd(batch, bySwitch, run, (r) => r.switch);
        } catch (error) {
          failure = failureOf(error);
          noteReamp(failure.reampStillOn, false);
          stampReamp(batch, { stillOn: failure.reampStillOn, warning: null });
        }
        sweepUnresolved(bySwitch, {
          reason: failure?.reason ?? null,
          kind: failure?.kind ?? "other",
        });
        continue;
      }

      // ── Scene rows: ONE backend call (one prepass + one runner) per batch. Each call
      // re-loads the preset, so per-scene dispatch flashed the unit back to base twice
      // per scene. Per-scene progress rides the command's channel.
      let cands = candCache.get(it.slot);
      if (!cands) {
        // Candidates come from the startup backup; a live block read is the fallback for
        // a preset the backup missed. That read can fail (the unit detached) — like any
        // per-item failure it skips this group with its reason and the run carries on.
        cands = deps.ampCandidates.get(it.slot);
        if (!cands) {
          try {
            cands = ampBlocks(await deps.listLevelBlocks(it.slot));
          } catch (error) {
            // The live read of the preset's blocks failed: the preset couldn't be read.
            skipBatch(runnable, {
              skipReason: errMsg(error),
              skipKind: "preset_unreadable",
            });
            continue;
          }
        }
        candCache.set(it.slot, cands);
      }
      // A row that names its own control (`handle`) needs no amp candidate. Bail locally
      // only when NOBODY in the group has one; a mixed group still dispatches and the
      // backend skips the amp-needing rows individually.
      if (cands.length === 0 && !runnable.some((r) => r.item.handle != null)) {
        skipBatch(runnable, { skipKind: "no_active_amp" });
        continue;
      }
      const byScene = new Map(runnable.map((r) => [r.item.sceneSlot, r.index]));
      const resolveScene = batchResolve(byScene, envelope);
      markGroupActive(byScene, i);
      // Keep this preset's BASE job alive through the batch's prepass + headroom-trade
      // phases (issue 1). The base row ran FIRST (base-first sort), so its target is known —
      // look it up in `work`, never the group; a Try again without the main sound anchors
      // on the earlier run's row. Omit the key entirely (not `null`) when no base row was
      // chosen, or its target is no longer configured (the run refused it): no trade to
      // plan.
      const baseItem =
        work.find((w) => w.slot === it.slot && w.isBase) ??
        deps.earlierBaseRows?.find((w) => w.slot === it.slot);
      const baseLufs = baseItem ? targetLufsByName(baseItem.targetName) : null;
      const baseAnchor =
        baseLufs != null ? { targetLufs: baseLufs } : undefined;
      // Per-scene outcomes arrive via the Channel, NOT the returned value — read only for
      // the run-end facts (the save's verdict and the re-amp warning). The offline e2e HTTP
      // bridge no-ops the Channel, so scene rows there resolve to "skipped" (see
      // level-defaults.spec.ts).
      let failure: ReturnType<typeof failureOf> | null = null;
      try {
        const run = await deps.levelScenesApplyBatched(
          {
            slot: it.slot,
            // `handle` is `Option`/serde-defaulted on the Rust side — an `undefined` value
            // is simply OMITTED by JSON serialization (never sent as `null`).
            jobs: runnable.map((r) => ({
              sceneSlot: r.item.sceneSlot ?? 0,
              targetLufs: r.targetLufs,
              handle: r.item.handle,
            })),
            candidates: cands,
            save: true,
            // A scene batch is one preset's rows, so its flag is that preset's choice.
            rebalance: deps.rebalance.has(it.slot),
            ...wireProfile,
            ...(baseAnchor ? { baseAnchor } : {}),
          },
          (item) => {
            resolveScene(
              item.sceneSlot,
              item.status,
              item.result,
              item.message,
              item.skipKind,
              item.tail,
            );
          },
        );
        noteReamp(run.reampStillOn, true);
        stampRunEnd(batch, byScene, run, (r) => r.scene_slot);
      } catch (error) {
        failure = failureOf(error);
        noteReamp(failure.reampStillOn, false);
        stampReamp(batch, { stillOn: failure.reampStillOn, warning: null });
      }
      sweepUnresolved(byScene, {
        reason: failure?.reason ?? null,
        kind: failure?.kind ?? "other",
      });
    }
  } catch (error) {
    // Anything the per-item handlers didn't catch: report it on every row still
    // unresolved, so the run ends in a terminal state the user can read.
    const reason = errMsg(error);
    const kind = errorSkipKind(error);
    work.forEach((w, j) => {
      if (w.status !== "result")
        patch(j, {
          status: "result",
          outcome: "skipped",
          skipReason: reason,
          skipKind: kind,
        });
    });
  }
  // Patches no step carried yet (a cancelled sweep's revert, a run-end stamp).
  if (pending.length > 0) publish(lastIndex);
  return { reampStillOn };
}
