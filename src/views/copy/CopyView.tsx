// src/views/copy/CopyView.tsx — the Copy tab: a 2-step flow + a save overlay.
//
// Step 1 (ChoosePresets) picks the reference + targets; Step 2 (PlaceBlocks) edits each
// target's signal path by hand (Replace / Insert / Remove from the reference's blocks)
// and saves. Everything is staged off-device; the unit is written ONLY on Save, one
// preset at a time via the live `copy_apply` command, gated on a per-preset CPU budget
// (76.5%) + the "I've backed up with Pro Control" checkbox.

import { useMemo, useState } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { LoadErrorPane } from "../LoadErrorPane";
import { errMsg, slotLabel } from "../../lib/format";
import { copyApply } from "../../lib/invoke";
import { patchLibraryGraph } from "../level/libraryScan";
import { patchLiveGraph, useLiveDevice } from "../level/useLiveDevice";
import { EmptyState, UsbC } from "../EmptyState";
import { ChoosePresets } from "./ChoosePresets";
import { PlaceBlocks } from "./PlaceBlocks";
import { SaveOverlay } from "./SaveOverlay";
import { type EditMode } from "./BlockEditor";
import { useCopyLibrary } from "./useCopyLibrary";
import {
  activeFromEditGraph,
  reconcileReadBack,
  applyEditOp,
  diffToOps,
  isEdited,
  removeEditBlock,
  retargetHistory,
  type EditHistory,
  type EditMap,
} from "./copyModel";
import {
  checkEdit,
  REASON_COPY,
  type BlockEditReason,
} from "./validateBlockEdit";
import type { ActiveGraph, CopyApplyItem, CopyJob } from "../../lib/types";

/** One target slot that can't currently be saved, and why. */
export interface BlockedSlot {
  slot: number;
  reason: BlockEditReason;
}

export interface CopyViewProps {
  connected: boolean;
  onScan?: () => void;
  /** The connect-time active graph — its slot marks the ON-UNIT preset. */
  initialGraph?: ActiveGraph | null;
}

export function CopyView({ connected, onScan, initialGraph }: CopyViewProps) {
  const { t } = useTheme();
  // The ON-UNIT marker follows the unit's LIVE active preset (the monitor pushes
  // hardware preset changes), not the frozen connect-time graph — so switching presets
  // on the unit moves the chip. Falls back to the connect-time slot before any live push
  // arrives (and under Vitest, where the event bridge is inert).
  const live = useLiveDevice(connected);
  const activeSlot = live.activeListIndex ?? initialGraph?.slot ?? null;
  const lib = useCopyLibrary(connected, activeSlot);
  const { presets, bySlot, ready } = lib;

  const [step, setStep] = useState<1 | 2>(1);
  const [fromSlot, setFromSlot] = useState<number | null>(null);
  const [toSet, setToSet] = useState<Set<number>>(new Set());
  // Offline undo/redo: a snapshot stack of the whole staged edit + a pointer. The
  // working copy is `edit = hist.stack[hist.idx]`. Local to the editing session — it
  // never touches the unit (the only device write is Save). It survives Back → Place
  // the blocks and a failed save; only a fully saved run clears it.
  const [hist, setHist] = useState<EditHistory | null>(null);
  const [open, setOpen] = useState<{ slot: number; uid: string } | null>(null);
  const [backedUp, setBackedUp] = useState(false);

  // Save-run state.
  const [saving, setSaving] = useState(false);
  const [saveSlots, setSaveSlots] = useState<number[]>([]);
  const [results, setResults] = useState<Map<number, CopyApplyItem>>(new Map());
  const [saveDone, setSaveDone] = useState(false);
  // Set when the whole copy_apply run REJECTS (vs per-preset errors in `results`).
  const [saveError, setSaveError] = useState<string | null>(null);

  const edit = hist ? hist.stack[hist.idx] : null;
  const canUndo = hist != null && hist.idx > 0;
  const canRedo = hist != null && hist.idx < hist.stack.length - 1;

  // Default the reference to the on-unit preset (else the first) once the list lands.
  // Conditional + converging (fromSlot becomes non-null), so it's a safe render-phase
  // adjustment rather than an effect.
  if (fromSlot == null && presets.length > 0) {
    const def = presets.find((p) => p.onUnit)?.slot ?? presets[0].slot;
    setFromSlot(def);
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
    setHist((h) => retargetHistory(h, toSlots, graphForSlot));
    setOpen(null);
    setStep(2);
  };

  // Push a new edit snapshot (truncating any redo branch); undo/redo move the pointer.
  const commitEdit = (updater: (m: EditMap) => EditMap): void => {
    setHist((h) => {
      if (!h) return h;
      const next = updater(h.stack[h.idx]);
      const stack = [...h.stack.slice(0, h.idx + 1), next];
      return { ...h, stack, idx: stack.length - 1 };
    });
  };
  const undo = (): void => {
    setHist((h) => (h && h.idx > 0 ? { ...h, idx: h.idx - 1 } : h));
  };
  const redo = (): void => {
    setHist((h) =>
      h && h.idx < h.stack.length - 1 ? { ...h, idx: h.idx + 1 } : h,
    );
  };

  // ── derived save gating (Step 2) ─────────────────────────────────────────
  // Recomputed only when an edit op lands (or the target set changes), not on every
  // render — `isEdited` + `checkEdit` walk every block of every target. `checkEdit`
  // covers all 5 firmware caps (CPU + the 4 count/coexistence rules) — this is the
  // UX-only up-front warning; the Rust `copy_apply` guard is the real enforcement.
  const { changedCount, blockedSlots, blockedSlotNums } = useMemo(() => {
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
    return {
      changedCount: changed,
      blockedSlots: blocked,
      blockedSlotNums: blocked.map((b) => b.slot),
    };
  }, [edit, toSlots]);
  const saveBlocked = !edit || changedCount === 0 || blockedSlots.length > 0;
  const hint =
    blockedSlots.length > 1
      ? `${String(blockedSlots.length)} presets can't save — fix them first.`
      : blockedSlots.length === 1
        ? `${nameOf(blockedSlots[0].slot)}: ${REASON_COPY[blockedSlots[0].reason]}.`
        : changedCount === 0
          ? "Tap a block in a preset to change it."
          : null;

  // ── edit handlers ────────────────────────────────────────────────────────
  const tapBlock = (slot: number, uid: string): void => {
    setOpen((o) => (o?.slot === slot && o.uid === uid ? null : { slot, uid }));
  };
  const applyOp = (
    slot: number,
    uid: string,
    mode: EditMode,
    model: string,
  ): void => {
    commitEdit((prev) => {
      const e = prev[slot];
      if (!e) return prev;
      return { ...prev, [slot]: applyEditOp(e, uid, mode, model) };
    });
    setOpen(null);
  };
  const removeOp = (slot: number, uid: string): void => {
    commitEdit((prev) => {
      const e = prev[slot];
      if (!e) return prev;
      return { ...prev, [slot]: removeEditBlock(e, uid) };
    });
    setOpen(null);
  };

  // ── save (the only device write) ─────────────────────────────────────────
  const runJobs = (jobs: CopyJob[]): void => {
    setSaveSlots(jobs.map((j) => j.listIndex));
    setResults(new Map());
    setSaveDone(false);
    setSaveError(null);
    setSaving(true);
    // Every result this run has seen: the streamed channel (online only) plus the
    // resolved list (the offline bridge streams nothing). A rejected run still keeps
    // the presets the channel already reported as saved.
    const seen = new Map<number, CopyApplyItem>();
    const onResult = (item: CopyApplyItem): void => {
      seen.set(item.slot, item);
      setResults(new Map(seen));
    };
    const settle = (items: CopyApplyItem[]): void => {
      for (const item of items) seen.set(item.slot, item);
      setResults(new Map(seen));
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
      setSaveDone(true);
    };
    void copyApply(jobs, true, onResult).then(settle, (e: unknown) => {
      // The whole run failed (device error / lost connection) — surface it instead of a
      // false "saved" (previously this swallowed the rejection and showed success).
      setSaveError(errMsg(e));
      settle([]);
    });
  };

  const onSave = (): void => {
    // `saveBlocked` already covers `edit == null`, so this guard narrows edit non-null.
    if (saveBlocked || !backedUp) return;
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
    // A preset is written when its result is "updated" (or "skipped": it had nothing to
    // write). An "error" row, or no row at all (a rejected or stopped run), was not saved.
    const written = saveSlots.filter((s) => {
      const r = results.get(s);
      return r != null && r.outcome !== "error";
    });
    setSaving(false);
    setSaveSlots([]);
    setResults(new Map());
    setSaveDone(false);
    setSaveError(null);
    setOpen(null);
    if (written.length === saveSlots.length) {
      // Everything saved: the unit now matches the edit, so discard the staged work and
      // return to Step 1.
      setHist(null);
      setBackedUp(false);
      setStep(1);
      return;
    }
    // Some or all presets did not save: stay on Step 2 with their staged edits (and
    // undo steps) intact. The written presets restart from their just-saved graph.
    // The backup acknowledgment stands — those presets are still as backed up.
    setHist((h) => retargetHistory(h, toSlots, graphForSlot, new Set(written)));
  };

  // ── render ────────────────────────────────────────────────────────────────
  if (!connected) {
    return (
      <EmptyState
        title="Copy lives on the Tone Master Pro"
        body={
          <>
            Connect your unit over <UsbC /> to copy blocks between presets. It
            connects on its own.
          </>
        }
        onScan={onScan}
      />
    );
  }

  if (lib.error != null) {
    return (
      <LoadErrorPane message={lib.error} onRetry={() => void lib.refresh()} />
    );
  }

  const from = fromSlot != null ? bySlot.get(fromSlot) : undefined;
  const activeSaveSlot = saveSlots.find((s) => !results.has(s)) ?? null;

  return (
    <div
      style={{
        position: "relative",
        flex: 1,
        minHeight: 0,
        display: "flex",
        flexDirection: "column",
        background: t.bg,
      }}
    >
      {step === 1 || !edit || !from ? (
        <ChoosePresets
          presets={presets}
          fromSlot={fromSlot}
          setFrom={setFromSlot}
          toSet={toSet}
          setTo={setToSet}
          ready={ready}
          scanning={lib.scanning}
          percent={lib.percent}
          onContinue={enterStep2}
        />
      ) : (
        <PlaceBlocks
          from={from}
          targets={toSlots}
          edit={edit}
          open={open}
          changedCount={changedCount}
          blockedSlots={blockedSlotNums}
          saveBlocked={saveBlocked}
          hint={hint}
          backedUp={backedUp}
          setBackedUp={setBackedUp}
          nameOf={nameOf}
          onBack={() => {
            setStep(1);
          }}
          onSave={onSave}
          onTapBlock={tapBlock}
          onApply={applyOp}
          onRemove={removeOp}
          onClose={() => {
            setOpen(null);
          }}
          onUndo={undo}
          onRedo={redo}
          canUndo={canUndo}
          canRedo={canRedo}
        />
      )}

      {saving && (
        <SaveOverlay
          slots={saveSlots}
          nameOf={nameOf}
          results={results}
          activeSlot={activeSaveSlot}
          done={saveDone}
          failed={saveError}
          onDone={onSaveDone}
        />
      )}
    </div>
  );
}

export default CopyView;
