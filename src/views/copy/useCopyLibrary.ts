// src/views/copy/useCopyLibrary.ts — the Copy view's read-path data layer.
//
// Builds the `CopyPreset[]` the Copy feature needs (slot + name + on-unit flag + the
// routed signal graph + real CPU) from the SAME module-scoped device backup the Presets
// tab uses (`libraryScan`), so opening Copy never triggers a second ~22 s read. The
// preset list paints instantly; each preset's graph fills in when the backup settles.

import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  useSyncExternalStore,
} from "react";

import { listPresets } from "../../lib/invoke";
import { useDeviceLoad } from "../../lib/useDeviceLoad";
import { presetCpu } from "../../models/cpu";
import { subscribeLibraryScan, getLibraryScan } from "../level/libraryScan";
import { isEmptyName } from "../level/usePresetData";
import type { ActiveGraph, PresetEntry } from "../../lib/types";

/** A preset the Copy feature can read from / write into. */
export interface CopyPreset {
  /** 0-based My-Presets list index. */
  slot: number;
  name: string;
  /** Currently loaded on the unit (the green ON UNIT chip). */
  onUnit: boolean;
  /** The routed signal graph (from the backup); empty when not yet loaded / unparseable. */
  graph: ActiveGraph;
  /** Real total DSP cost (% of budget), or null when no graph yet. */
  cpu: number | null;
}

const EMPTY_GRAPH: ActiveGraph = {
  name: null,
  slot: null,
  template: null,
  split_mix: null,
  nodes: [],
  stages: [],
};

export interface CopyLibrary {
  presets: CopyPreset[];
  /** `presets` keyed by slot for O(1) lookups (same identity as `presets`). */
  bySlot: Map<number, CopyPreset>;
  /** List read settled. */
  loaded: boolean;
  /** Backup graph load settled (graphs available). */
  ready: boolean;
  scanning: boolean;
  percent: number;
  error: string | null;
  refresh: () => Promise<void>;
}

export function useCopyLibrary(
  connected: boolean,
  activeSlot: number | null,
): CopyLibrary {
  const { phase, mountedRef, runLoad } = useDeviceLoad();
  const [rows, setRows] = useState<PresetEntry[]>([]);

  const lib = useSyncExternalStore(subscribeLibraryScan, getLibraryScan);

  const refresh = useCallback(async () => {
    await runLoad(async () => {
      const list = await listPresets();
      if (!mountedRef.current) return;
      setRows(list);
    });
  }, [runLoad, mountedRef]);

  useEffect(() => {
    if (connected) void refresh();
  }, [connected, refresh]);

  // The backup scan is App-owned (one per connection, shared by every device tab); this
  // hook only SUBSCRIBES to it (`lib`) — no trigger here, so a tab switch never re-scans.

  // Rebuilt only when the list, the backup graphs, or the on-unit slot change — not on
  // every render (a parent re-render / search keystroke). `lib` is a stable ref between
  // scans (useSyncExternalStore), so the per-preset `presetCpu` node-walk over the whole
  // library runs once per scan settle, not per keystroke.
  const presets = useMemo<CopyPreset[]>(
    () =>
      rows
        .filter((p) => !isEmptyName(p.name))
        .map((p) => {
          const graph = lib.graphByIndex.get(p.slot) ?? EMPTY_GRAPH;
          return {
            slot: p.slot,
            name: p.name,
            onUnit: activeSlot != null && p.slot === activeSlot,
            graph,
            cpu: lib.ready ? presetCpu(graph) : null,
          };
        }),
    [rows, lib.graphByIndex, lib.ready, activeSlot],
  );

  const bySlot = useMemo(
    () => new Map(presets.map((p) => [p.slot, p])),
    [presets],
  );

  return {
    presets,
    bySlot,
    loaded: phase.kind === "ready",
    ready: lib.ready,
    scanning: lib.scanning,
    percent: lib.percent,
    error: phase.kind === "error" ? phase.message : null,
    refresh,
  };
}
