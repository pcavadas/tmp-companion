// src/views/settings/useTargets.ts — the loudness-target list operations: create /
// rename / set level / reorder (⋯ Move up/down, or drag) / delete. Each persists the
// whole list through the store's optimistic `persistTargets`; a level drag updates
// locally per move and persists ONCE on release.

import { useState } from "react";

import type { SettingsStore, UiTarget } from "./useSettingsStore";

export interface TargetOps {
  /** uid of a freshly added target → it opens directly in rename mode. */
  justAdded: string | null;
  addTarget: () => void;
  renameTarget: (uid: string, name: string) => void;
  deleteTarget: (uid: string) => void;
  moveTarget: (uid: string, dir: -1 | 1) => void;
  reorderTargets: (fromUid: string | null, toUid: string) => void;
  setTargetLevelLocal: (uid: string, lufs: number) => void;
  commitTargetLevel: (uid: string, lufs: number) => void;
}

const withLevel = (rows: UiTarget[], uid: string, lufs: number) =>
  rows.map((row) => (row.uid === uid ? { ...row, lufs } : row));

export function useTargets({
  targets,
  setTargets,
  persistTargets,
  newUid,
}: Pick<
  SettingsStore,
  "targets" | "setTargets" | "persistTargets" | "newUid"
>): TargetOps {
  const [justAdded, setJustAdded] = useState<string | null>(null);

  function addTarget() {
    const uid = newUid();
    // PR2 re-baseline: +3 from the mono-era -22.0 default (same audible loudness
    // under the new 2-ch BS.1770 convention — see TargetRow's TMIN/TMAX doc).
    void persistTargets([...targets, { uid, name: "New target", lufs: -19.0 }]);
    setJustAdded(uid);
  }
  function renameTarget(uid: string, name: string) {
    void persistTargets(
      targets.map((row) => (row.uid === uid ? { ...row, name } : row)),
    );
    if (justAdded === uid) setJustAdded(null);
  }
  function deleteTarget(uid: string) {
    void persistTargets(targets.filter((row) => row.uid !== uid));
  }
  function moveTarget(uid: string, dir: -1 | 1) {
    const i = targets.findIndex((r) => r.uid === uid);
    const j = i + dir;
    if (i < 0 || j < 0 || j >= targets.length) return;
    const next = [...targets];
    [next[i], next[j]] = [next[j], next[i]];
    void persistTargets(next);
  }
  function reorderTargets(fromUid: string | null, toUid: string) {
    if (!fromUid || fromUid === toUid) return;
    const next = [...targets];
    const fi = next.findIndex((r) => r.uid === fromUid);
    const ti = next.findIndex((r) => r.uid === toUid);
    if (fi < 0 || ti < 0) return;
    const [item] = next.splice(fi, 1);
    next.splice(ti, 0, item);
    void persistTargets(next);
  }
  // Optimistic per-move update (drives the fill/readout); does NOT persist.
  function setTargetLevelLocal(uid: string, lufs: number) {
    setTargets((prev) => withLevel(prev, uid, lufs));
  }
  // Persist ONCE on pointer-release (not on every drag move).
  function commitTargetLevel(uid: string, lufs: number) {
    void persistTargets(withLevel(targets, uid, lufs));
  }

  return {
    justAdded,
    addTarget,
    renameTarget,
    deleteTarget,
    moveTarget,
    reorderTargets,
    setTargetLevelLocal,
    commitTargetLevel,
  };
}
