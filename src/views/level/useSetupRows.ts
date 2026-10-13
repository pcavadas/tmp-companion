// src/views/level/useSetupRows.ts — the Set up page's per-row state: each sound's
// Guitar / Target / handle / scene-context picks, the bulk row that writes every sound,
// the candidate fetches behind the handle pickers, and the commit (`buildChoices`).
//
// A re-run row carries the instrument and target it was leveled with; seed from them so
// "Run again" repeats the same settings instead of resetting to the first profile and
// target. Only ids the pickers still offer are kept: a `Pick` whose value is missing from
// its options DISPLAYS `options[0]` while submitting the stale id (danger.md).

import { useEffect, useMemo, useState } from "react";

import type { PickOption } from "../overlays/Pick";
import type { FlatLevelFetch } from "./FlatLevelPick";
import type { BlockLevelHandle } from "./blockLevelGroups";
import { enablingBlockHandle, sceneCandidatesFetch } from "./setupCandidates";
import { useSceneHandles } from "./useSceneHandles";
import { useLevelBlocks } from "./useLevelBlocks";
import {
  useFootswitchSceneContexts,
  type FsContextFetchState,
} from "./useFootswitchSceneContexts";
import {
  buildSetupChoices,
  type SetupChoice,
  type SetupOption,
  type SetupRowPick,
} from "./leveling";

/** One sound's picks. `handle` undefined = not chosen (the page preselects). */
export interface RowChoice extends SetupRowPick {
  handle: BlockLevelHandle | null | undefined;
}

/** One preset of the Set up list, its sounds in option order. */
export interface SetupGroup {
  slot: number;
  name: string;
  opts: SetupOption[];
}

/** `id` when the picker still offers it, else `fallback`. */
function seedFrom(
  id: string | undefined,
  options: PickOption[],
  fallback: string,
): string {
  return id !== undefined && options.some((x) => x.id === id) ? id : fallback;
}

/** The one value every entry shares, else `fallback`. */
function sharedOr(values: string[], fallback: string): string {
  const distinct = new Set(values);
  if (distinct.size !== 1) return fallback;
  const [only] = distinct;
  return only;
}

export interface UseSetupRowsArgs {
  options: SetupOption[];
  isRelevel: boolean;
  instrumentOptions: PickOption[];
  targetOptions: PickOption[];
  defaultInst: string;
  defaultTarget: string;
}

export function useSetupRows({
  options,
  isRelevel,
  instrumentOptions,
  targetOptions,
  defaultInst,
  defaultTarget,
}: UseSetupRowsArgs) {
  const groups = useMemo(() => {
    const by = new Map<number, SetupGroup>();
    options.forEach((o) => {
      let group = by.get(o.slot);
      if (!group) {
        group = { slot: o.slot, name: o.presetName, opts: [] };
        by.set(o.slot, group);
      }
      group.opts.push(o);
    });
    return [...by.values()].sort((a, b) => a.slot - b.slot);
  }, [options]);

  const seedInst = (o: SetupOption): string =>
    seedFrom(o.instId, instrumentOptions, defaultInst);
  const seedTarget = (o: SetupOption): string =>
    seedFrom(o.targetName, targetOptions, defaultTarget);

  const [rows, setRows] = useState<Partial<Record<string, RowChoice>>>(() => {
    const m: Partial<Record<string, RowChoice>> = {};
    options.forEach((o) => {
      m[o.key] = {
        inst: seedInst(o),
        target: seedTarget(o),
        handle:
          o.footswitch != null
            ? {
                groupId: o.footswitch.levGroupId,
                nodeId: o.footswitch.levNodeId,
                parameterId: o.footswitch.levParameterId,
              }
            : o.isBase
              ? (o.baseHandle ?? null)
              : (o.sceneHandle ?? undefined),
        // A re-run footswitch keeps the scene it was measured in. A fresh row stays
        // undefined and resolves to the preset's suggested scene (`fsSuggestedFor`).
        sceneContext:
          isRelevel && o.footswitch != null
            ? o.footswitch.sceneContext
            : undefined,
      };
    });
    return m;
  });
  const patchRow = (k: string, partial: Partial<RowChoice>) => {
    setRows((p) => {
      const cur = p[k];
      if (!cur) return p;
      return { ...p, [k]: { ...cur, ...partial } };
    });
  };

  const {
    prefetch: fetchHandlesFor,
    candidatesFor,
    hasBackupData: sceneHasBackup,
  } = useSceneHandles();
  const {
    prefetch: fetchBlocksFor,
    blocksFor,
    hasBackupData: baseHasBackup,
  } = useLevelBlocks();
  const { prefetch: fetchFsContextFor, contextFor } =
    useFootswitchSceneContexts();

  useEffect(() => {
    groups.forEach((g) => {
      if (baseHasBackup(g.slot)) fetchBlocksFor(g.slot);
      if (sceneHasBackup(g.slot)) fetchHandlesFor(g.slot);
    });
  }, [groups, fetchBlocksFor, fetchHandlesFor, baseHasBackup, sceneHasBackup]);

  // A footswitch row's default scene context is the preset's suggested scene, so resolve
  // it for every footswitch row up front rather than on first picker open — otherwise the
  // same selection could measure a switch in different scenes depending on whether a
  // picker was opened. Answered off the backup scan; a device read only for a preset the
  // scan has no entry for. Rows that already carry a context (a re-run) need no fetch.
  useEffect(() => {
    if (isRelevel) return;
    options.forEach((o) => {
      if (o.footswitch != null) fetchFsContextFor(o.slot);
    });
  }, [options, isRelevel, fetchFsContextFor]);

  const effectiveHandle = (
    o: SetupOption,
    row: RowChoice | undefined,
    // Pass this when the caller already built the row's `FlatLevelFetch` (e.g. for
    // the `candidates` prop) — skips re-deriving it from raw fetch state.
    fetch?: FlatLevelFetch,
  ): BlockLevelHandle | null => {
    if (row?.handle !== undefined) return row.handle;
    if (o.footswitch == null && o.sceneSlot != null) {
      const st = candidatesFor(o.slot, o.sceneSlot);
      const preselect = enablingBlockHandle(
        st,
        fetch ?? sceneCandidatesFetch(st),
      );
      if (preselect) return preselect;
    }
    return null;
  };

  const fsSuggestedFor = (o: SetupOption): number | null => {
    if (o.footswitch == null) return null;
    const st = contextFor(o.slot, o.footswitch.switchIndex);
    return st.status === "resolved" ? (st.row?.suggested ?? null) : null;
  };

  // A footswitch row with no scene context the user chose, and whose suggested one is
  // still being read or could not be read. Start waits for the read, so the run never
  // measures a switch in base just because it hadn't landed yet; a FAILED read (no
  // retry within this mount, see `useLazySlotCache`) needs an explicit pick instead.
  const fsContextState = (
    o: SetupOption,
  ): FsContextFetchState["status"] | null =>
    o.footswitch == null || rows[o.key]?.sceneContext !== undefined
      ? null
      : contextFor(o.slot, o.footswitch.switchIndex).status;
  const fsContextPending = options.some((o) => {
    const st = fsContextState(o);
    return st === "unfetched" || st === "loading";
  });
  // Only a row that renders the scene picker can take that pick.
  const fsContextMissing = options.some(
    (o) => fsContextState(o) === "error" && (o.levelParams ?? []).length > 0,
  );

  // The bulk row's current value — always written to EVERY row (no ticking; what
  // gets leveled was already chosen on the preset list). Seeded with the rows' shared
  // value when they all agree (a re-run of sounds leveled alike), else the default.
  const [bulkInst, setBulkInst] = useState(() =>
    sharedOr(options.map(seedInst), defaultInst),
  );
  const [bulkTarget, setBulkTarget] = useState(() =>
    sharedOr(options.map(seedTarget), defaultTarget),
  );
  const applyBulkField = (field: "inst" | "target", v: string) => {
    setRows((p) => {
      const n = { ...p };
      options.forEach((o) => {
        const cur = n[o.key];
        if (cur) n[o.key] = { ...cur, [field]: v };
      });
      return n;
    });
  };
  const applyBulkInst = (v: string) => {
    setBulkInst(v);
    applyBulkField("inst", v);
  };
  const applyBulkTarget = (v: string) => {
    setBulkTarget(v);
    applyBulkField("target", v);
  };

  /** The run's choices from the rows as they stand (the Start commit). */
  const buildChoices = (): SetupChoice[] =>
    buildSetupChoices(
      options,
      (k) => rows[k],
      (o) => effectiveHandle(o, rows[o.key]),
      fsSuggestedFor,
      { inst: defaultInst, target: defaultTarget },
    );

  return {
    groups,
    rows,
    patchRow,
    bulkInst,
    bulkTarget,
    applyBulkInst,
    applyBulkTarget,
    effectiveHandle,
    fsSuggestedFor,
    fsContextState,
    fsContextPending,
    fsContextMissing,
    candidatesFor,
    blocksFor,
    contextFor,
    fetchBlocksFor,
    fetchHandlesFor,
    fetchFsContextFor,
    buildChoices,
  };
}
