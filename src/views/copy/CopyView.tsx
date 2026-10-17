// src/views/copy/CopyView.tsx — the Copy tab: a 2-step flow + a save overlay (look B).
//
// Step 1 (ChoosePresets) picks the preset to copy from + the targets; Step 2 (PlaceBlocks)
// edits each target's signal path by hand — choose a block on the source strip, then a
// "+" gap in each target (insert), or a target block's menu (Replace / Remove) — and
// saves. Everything is staged off-device; the unit is written ONLY on Save, one changed
// preset at a time via the live `copy_apply` command, gated on the firmware limits
// (`checkEdit`) + the "I've backed up with Pro Control" tick. The flow's state is one
// reducer (./copyFlow): choose | edit | saving, the staged work riding all three.

import { useMemo, useReducer } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { LoadErrorPane } from "../LoadErrorPane";
import { slotLabel } from "../../lib/format";
import { copyApply, logUnlessCommandError } from "../../lib/invoke";
import { logError } from "../../lib/log";
import { patchLibraryGraph } from "../../device/libraryScan";
import { patchLiveGraph, useLiveDevice } from "../../device/useLiveDevice";
import { DeviceState } from "../DeviceState";
import type { NoUnitState } from "../../lib/connectError";
import { ChoosePresets } from "./ChoosePresets";
import { PlaceBlocks } from "./PlaceBlocks";
import { SaveOverlay } from "./SaveOverlay";
import { useCopyLibrary } from "./useCopyLibrary";
import {
  activeFromEditGraph,
  reconcileReadBack,
  diffToOps,
  isEdited,
  retargetHistory,
} from "./copyModel";
import { copyReducer, currentEdit, INITIAL_COPY_STATE } from "./copyFlow";
import { checkEdit, type BlockedSlot } from "./validateBlockEdit";
import { saveReason } from "./copyText";
import type { ActiveGraph, CopyApplyItem, CopyJob } from "../../lib/types";

export interface CopyViewProps {
  connected: boolean;
  /** What to show while no unit is connected (App's connect state). Default "off". */
  deviceState?: NoUnitState;
  /** The connect-time active graph — its slot marks the ON-UNIT preset. */
  initialGraph?: ActiveGraph | null;
}

export function CopyView({
  connected,
  deviceState = "off",
  initialGraph,
}: CopyViewProps) {
  const { t } = useTheme();
  // The "Loaded" badge follows the unit's LIVE active preset (the monitor pushes
  // hardware preset changes), not the frozen connect-time graph — so switching presets
  // on the unit moves the badge. Falls back to the connect-time slot before any live push
  // arrives (and under Vitest, where the event bridge is inert).
  const live = useLiveDevice(connected);
  const activeSlot = live.activeListIndex ?? initialGraph?.slot ?? null;
  const lib = useCopyLibrary(connected, activeSlot);
  const { presets, bySlot, ready } = lib;

  const [state, dispatch] = useReducer(copyReducer, INITIAL_COPY_STATE);
  const { fromSlot, toSet, phase } = state;
  const edit = currentEdit(phase);
  const hist = phase.draft.hist;
  const canUndo = hist != null && hist.idx > 0;
  const canRedo = hist != null && hist.idx < hist.stack.length - 1;

  // Default the reference to the on-unit preset (else the first) once the list lands.
  // Conditional + converging (fromSlot becomes non-null), so it's a safe render-phase
  // adjustment rather than an effect.
  if (fromSlot == null && presets.length > 0) {
    const def = presets.find((p) => p.onUnit)?.slot ?? presets[0].slot;
    dispatch({ type: "setFrom", slot: def });
  }

  const nameOf = (slot: number): string =>
    bySlot.get(slot)?.name ?? slotLabel(slot);
  const graphForSlot = (slot: number): ActiveGraph | null =>
    bySlot.get(slot)?.graph ?? null;

  // The chosen targets, in list order.
  const toSlots = useMemo(
    () => presets.filter((p) => toSet.has(p.slot)).map((p) => p.slot),
    [presets, toSet],
  );

  // Re-entering Step 2 keeps the staged edits (and their undo steps) of every target
  // still selected; only a newly picked target starts from its library graph.
  const enterStep2 = (): void => {
    dispatch({
      type: "enterEdit",
      hist: retargetHistory(hist, toSlots, graphForSlot),
    });
  };

  // ── derived save gating (Step 2) ─────────────────────────────────────────
  // Recomputed only when an edit op lands (or the target set changes), not on every
  // render — `isEdited` + `checkEdit` walk every block of every target. `checkEdit`
  // covers all 5 firmware caps (CPU + the 4 count/coexistence rules) — this is the
  // UX-only up-front warning; the Rust `copy_apply` guard is the real enforcement.
  const { changedCount, blockedSlots } = useMemo(() => {
    let changed = 0;
    const blocked: BlockedSlot[] = [];
    if (edit) {
      for (const s of toSlots) {
        const e = edit[s];
        if (!e) continue;
        if (isEdited(e)) changed += 1;
        const reason = checkEdit(e);
        if (reason) blocked.push({ slot: s, reason });
      }
    }
    return { changedCount: changed, blockedSlots: blocked };
  }, [edit, toSlots]);
  const saveBlocked = !edit || changedCount === 0 || blockedSlots.length > 0;
  const reason = saveReason(
    blockedSlots.map((b) => ({ name: nameOf(b.slot), reason: b.reason })),
    changedCount,
  );

  // ── save (the only device write) ─────────────────────────────────────────
  const runJobs = (jobs: CopyJob[]): void => {
    dispatch({ type: "saveStart", slots: jobs.map((j) => j.listIndex) });
    // Every result this run has seen: the streamed channel (online only) plus the
    // resolved list (the offline bridge streams nothing). A rejected run still keeps
    // the presets the channel already reported as saved.
    const seen = new Map<number, CopyApplyItem>();
    const onResult = (item: CopyApplyItem): void => {
      seen.set(item.slot, item);
      dispatch({ type: "saveResults", results: new Map(seen) });
    };
    const settle = (items: CopyApplyItem[]): void => {
      for (const item of items) seen.set(item.slot, item);
      dispatch({ type: "saveResults", results: new Map(seen) });
      // The screen says "Couldn't save"; why is for the log (and so a bug report).
      for (const item of seen.values()) {
        if (item.outcome === "error")
          logError(`copy save: ${item.name} wasn't saved: ${item.detail}`);
      }
      // Patch the cached library graph in place so a second edit reads the just-saved
      // path WITHOUT a ~22 s backup re-scan — an edit NEVER triggers a refetch. The edit
      // the device acked is the truth (optimistic); the device's working-copy read
      // (taken by the backend BEFORE the save) is adopted only when it shows those same
      // blocks (`reconcileReadBack`) — the earlier buffer-scraped read-back once handed
      // back the LOAD-TIME graph and patched the cache back to the pre-edit blocks.
      // Done once the run settles, from every result seen. ONLY for a CONFIRMED save: a "skipped"/"error" item (device
      // rejected the edit) must NOT patch the cache, or it would assert blocks the unit
      // never saved.
      for (const item of seen.values()) {
        if (item.outcome !== "updated") continue;
        const staged = edit?.[item.slot];
        if (!staged) continue;
        const patch = reconcileReadBack(
          activeFromEditGraph(
            staged.graph,
            graphForSlot(item.slot) ?? undefined,
          ),
          item.graph,
        );
        patchLibraryGraph(item.slot, patch);
        // BUG-2: a live block edit pushes no device field-3, so optimistically
        // repaint the hero when we just edited the ACTIVE preset (no device re-read).
        if (item.slot === activeSlot) patchLiveGraph(patch);
      }
      dispatch({ type: "saveSettled" });
    };
    void copyApply(jobs, true, onResult).then(settle, (e: unknown) => {
      // The whole run failed (device error / lost connection): the presets with no
      // result read as not saved — never a false "saved" — and the cause goes to the log.
      logUnlessCommandError("copy save", e);
      settle([]);
    });
  };

  const onSave = (): void => {
    // `saveBlocked` already covers `edit == null`, so this guard narrows edit non-null.
    if (saveBlocked || !phase.draft.backedUp) return;
    const jobs: CopyJob[] = toSlots
      .map((s): CopyJob | null => {
        const e = edit[s];
        const ops = e ? diffToOps(e) : [];
        return ops.length > 0 ? { listIndex: s, name: nameOf(s), ops } : null;
      })
      .filter((j): j is CopyJob => j != null);
    if (jobs.length === 0) return;
    runJobs(jobs);
  };

  const onSaveDone = (): void => {
    if (phase.kind !== "saving") return;
    const { slots, results } = phase;
    // A preset is written when its result is "updated" (or "skipped": it had nothing to
    // write). An "error" row, or no row at all (a rejected or stopped run), was not saved.
    const written = slots.filter((s) => {
      const r = results.get(s);
      return r != null && r.outcome !== "error";
    });
    // Some or all presets did not save: the written presets restart from their
    // just-saved graph; every other staged edit (and its undo steps) stays.
    dispatch({
      type: "saveDismiss",
      hist:
        written.length === slots.length
          ? null
          : retargetHistory(hist, toSlots, graphForSlot, new Set(written)),
    });
  };

  // ── render ────────────────────────────────────────────────────────────────
  if (!connected) return <DeviceState state={deviceState} />;

  if (lib.error != null) {
    return <LoadErrorPane what="presets" onRetry={() => void lib.refresh()} />;
  }

  const from = fromSlot != null ? bySlot.get(fromSlot) : undefined;
  const saving = phase.kind === "saving" ? phase : null;
  const activeSaveSlot = saving
    ? (saving.slots.find((s) => !saving.results.has(s)) ?? null)
    : null;
  // The ticked presets the save skips: nothing changed in them.
  const unchanged = saving
    ? toSlots.filter((s) => !saving.slots.includes(s)).map(nameOf)
    : [];

  return (
    <div
      style={{
        position: "relative",
        flex: 1,
        minHeight: 0,
        display: "flex",
        flexDirection: "column",
        background: t.window,
      }}
    >
      {phase.kind === "choose" || !edit || !from ? (
        <ChoosePresets
          presets={presets}
          listLoading={lib.listLoading}
          fromSlot={fromSlot}
          setFrom={(slot) => {
            dispatch({ type: "setFrom", slot });
          }}
          toSet={toSet}
          setTo={(next) => {
            dispatch({ type: "setTo", next });
          }}
          ready={ready}
          scanning={lib.scanning}
          onContinue={enterStep2}
        />
      ) : (
        <PlaceBlocks
          from={from}
          targets={toSlots}
          edit={edit}
          pick={phase.pick}
          open={phase.open}
          backedUp={phase.draft.backedUp}
          gate={{
            changedCount,
            blocked: blockedSlots,
            saveBlocked,
            reason,
          }}
          canUndo={canUndo}
          canRedo={canRedo}
          nameOf={nameOf}
          onSave={onSave}
          dispatch={dispatch}
        />
      )}

      {saving && (
        <SaveOverlay
          slots={saving.slots}
          nameOf={nameOf}
          results={saving.results}
          activeSlot={activeSaveSlot}
          done={saving.done}
          unchanged={unchanged}
          onDone={onSaveDone}
        />
      )}
    </div>
  );
}
