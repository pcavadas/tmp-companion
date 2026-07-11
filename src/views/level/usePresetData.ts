// src/views/level/usePresetData.ts — the Presets-view read-path data layer.
//
// Owns the My-Presets list, the profile/target store, the row selection, and the
// derived profile/target lookups — plus the connected-edge refresh. The active
// graph is NOT owned here (it arrives from the combined handshake + live monitor).

import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";

import { listPresets, getStore } from "../../lib/invoke";
import { useDeviceLoad } from "../../lib/useDeviceLoad";
import { baseKey, childKeys } from "./leveling";
import { subscribeLibraryScan, getLibraryScan } from "./libraryScan";
import type { PresetRow } from "../PresetList";
import type {
  PresetEntry,
  Store,
  Profile,
  SceneInfo,
  FootswitchInfo,
} from "../../lib/types";

/** Parse the owning 0-based list index out of a scene key (`p3` / `s3:1` → 3). */
function slotOfKey(key: string): number {
  const body = key.slice(1); // drop the `p` / `s` prefix
  const colon = body.indexOf(":");
  return parseInt(colon === -1 ? body : body.slice(0, colon), 10);
}

/** Two-phase load: the preset list paints instantly, then the WHOLE library's scene
 * details stream in via ONE device backup (~22 s) — `read_library_via_backup`, whose
 * `tmp://backup-progress` events drive the scan strip. `scanning` is true for that
 * window; `percent` is the determinate transfer %. */
export interface ScanState {
  scanning: boolean;
  percent: number;
}

// The list's empty-slot marker (CLAUDE.md): "--" / "Empty" / "—".
export function isEmptyName(name: string): boolean {
  const n = name.trim();
  return n === "" || n === "--" || n === "—" || n.toLowerCase() === "empty";
}

export function toPresetRow(p: PresetEntry): PresetRow {
  return { slot: p.slot, name: p.name, empty: isEmptyName(p.name) };
}

export function usePresetData(connected: boolean) {
  const { phase, setPhase, mountedRef, runLoad } = useDeviceLoad();
  const [rows, setRows] = useState<PresetRow[]>([]);
  const [store, setStore] = useState<Store | null>(null);
  // SELECTION is a flat set of scene KEYS (Base = `p${slot}`, FS = `s${slot}:${i}`).
  // A whole-preset tick adds all of a preset's child keys; an individual scene tick
  // (only reachable after the caret expands, i.e. after `ready`) flips one key.
  const [sel, setSel] = useState<Set<string>>(new Set());
  // Presets ticked WHOLE while their scenes were still unknown (during the backup
  // load, when carets are inert). Reconciled into `sel` — all child keys once scenes
  // arrive, or just the Base key if the backup missed that preset — when `ready` flips.
  const [pendingWhole, setPendingWhole] = useState<Set<number>>(new Set());
  // Which preset slots are expanded to show their scene sub-rows.
  const [expanded, setExpanded] = useState<Set<number>>(new Set());

  // The background scene read lives in a MODULE-SCOPED controller so it runs ONCE per
  // device connection and survives LevelView unmount/remount — switching tabs never
  // re-triggers the ~22 s backup. `ready` flips when it settles; `sceneInfo` is keyed
  // by 0-based LIST INDEX (= backup device slot − 1).
  const lib = useSyncExternalStore(subscribeLibraryScan, getLibraryScan);
  const { sceneInfo, ready } = lib;
  const footswitchInfo = lib.footswitchesPerIndex;
  const scan: ScanState = { scanning: lib.scanning, percent: lib.percent };

  // rows + sceneInfo + footswitchInfo in refs so the stable toggle callbacks see the
  // latest data. Synced in an effect (after commit) rather than during render — the
  // callbacks only read these when they fire, which is always after a commit.
  const rowsRef = useRef<PresetRow[]>(rows);
  const sceneInfoRef = useRef<Map<number, SceneInfo[]>>(sceneInfo);
  const footswitchInfoRef =
    useRef<Map<number, FootswitchInfo[]>>(footswitchInfo);
  useEffect(() => {
    rowsRef.current = rows;
    sceneInfoRef.current = sceneInfo;
    footswitchInfoRef.current = footswitchInfo;
  });

  const profileByName = useCallback(
    (name: string | null): Profile | null =>
      name ? (store?.profiles.find((p) => p.name === name) ?? null) : null,
    [store],
  );

  // Case-insensitive lookup by target NAME (the leveling flow carries the display
  // name, e.g. "Crunch"); −30 is the conservative fallback when a name is unknown.
  const targetLufsByName = useCallback(
    (name: string | null): number =>
      store?.targets.find((tg) => tg.name.toLowerCase() === name?.toLowerCase())
        ?.lufs ?? -30,
    [store],
  );

  // ── read-only loads (mount + after connect) ───────────────────────────────
  // The active graph is no longer fetched here — it arrives from the combined
  // handshake (initialGraph) and the live monitor (tmp://signal-chain), so the
  // separate readActivePreset round-trip (which doubled the connect time) is gone.
  const refresh = useCallback(async () => {
    await runLoad(async () => {
      const [list, st] = await Promise.all([listPresets(), getStore()]);
      if (!mountedRef.current) return;
      const mapped = list.map(toPresetRow);
      setRows(mapped);
      setStore(st);
      const slots = new Set(mapped.map((p) => p.slot));
      setSel((prev) => {
        const next = new Set<string>();
        prev.forEach((k) => {
          if (slots.has(slotOfKey(k))) next.add(k);
        });
        return next;
      });
      setPendingWhole((prev) => {
        const next = new Set<number>();
        prev.forEach((s) => {
          if (slots.has(s)) next.add(s);
        });
        return next;
      });
    });
  }, [runLoad, mountedRef]);

  // Load (and re-load) only once the device is connected. App owns the real
  // connection state and threads it down; the rising edge re-runs refresh.
  useEffect(() => {
    if (connected) void refresh();
  }, [connected, refresh]);

  // ── two-phase scene load (the whole library via ONE device backup) ─────────
  // The scan itself is App-owned (started once on connect, reset on disconnect) so
  // every device tab consumes the SAME one-shot scan; this hook only SUBSCRIBES to it
  // (`lib` above) and reconciles whole-preset ticks against it below.

  // ── reconcile whole-preset ticks made during the load ──────────────────────
  // Once scenes are known (or the load gave up), fold each `pendingWhole` slot into
  // `sel` as real scene keys: all child keys when scenes arrived, just the Base key
  // when the backup missed that preset (Base-only is the cross-preset essential).
  // Done during render (it converges in one pass — `pendingWhole` is emptied) rather
  // than in an effect, so the un-reconciled selection never paints.
  if (ready && pendingWhole.size > 0) {
    setSel((prev) => {
      const next = new Set(prev);
      pendingWhole.forEach((slot) => {
        const scenes = sceneInfo.get(slot);
        const keys = scenes
          ? childKeys(slot, scenes, footswitchInfo.get(slot) ?? [])
          : [baseKey(slot)];
        keys.forEach((k) => next.add(k));
      });
      return next;
    });
    setPendingWhole(new Set());
  }

  // ── selection ─────────────────────────────────────────────────────────────
  // Tick a WHOLE preset: when its scenes are known, toggle all child keys (all-on ⇒
  // clear, else select all). When still unknown (mid-load), track it in pendingWhole
  // — the reconcile effect folds it into `sel` once scenes land. Reads fresh
  // rows/sceneInfo from refs so the callbacks stay stable.
  const togglePreset = useCallback((slot: number) => {
    const row = rowsRef.current.find((r) => r.slot === slot);
    if (!row || row.empty) return;
    const scenes = sceneInfoRef.current.get(slot);
    if (scenes === undefined) {
      setPendingWhole((prev) => {
        const next = new Set(prev);
        if (next.has(slot)) next.delete(slot);
        else next.add(slot);
        return next;
      });
      return;
    }
    const keys = childKeys(
      slot,
      scenes,
      footswitchInfoRef.current.get(slot) ?? [],
    );
    setSel((prev) => {
      const next = new Set(prev);
      const allOn = keys.every((k) => next.has(k));
      keys.forEach((k) => (allOn ? next.delete(k) : next.add(k)));
      return next;
    });
  }, []);

  // Tick one scene key (Base or an FS scene — only reachable after the caret expands).
  const toggleKey = useCallback((key: string) => {
    setSel((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }, []);

  const toggleExpand = useCallback((slot: number) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(slot)) next.delete(slot);
      else next.add(slot);
      return next;
    });
  }, []);

  const clearSelection = useCallback(() => {
    setSel(new Set());
    setPendingWhole(new Set());
  }, []);

  // Drop just the given scene keys from the selection (BUG-4: the leveling flow prunes
  // exactly what a run leveled, leaving un-run sounds ticked).
  const deselectKeys = useCallback((keys: string[]) => {
    if (keys.length === 0) return;
    setSel((prev) => {
      const next = new Set(prev);
      keys.forEach((k) => next.delete(k));
      return next;
    });
  }, []);

  // Select-all toggle (header checkbox): every scene key of every non-empty preset ↔
  // none. Reads fresh rows/sceneInfo from the refs so it isn't stale.
  const toggleAll = useCallback(() => {
    const named = rowsRef.current.filter((r) => !r.empty);
    const everyKey: string[] = named.flatMap((r) =>
      childKeys(
        r.slot,
        sceneInfoRef.current.get(r.slot) ?? [],
        footswitchInfoRef.current.get(r.slot) ?? [],
      ),
    );
    setSel((prev) => {
      const all = everyKey.length > 0 && everyKey.every((k) => prev.has(k));
      return all ? new Set() : new Set(everyKey);
    });
    setPendingWhole(new Set());
  }, []);

  // ── derived selection counts (memoized — only recompute when the selection
  //    actually changes, not on every render of the consuming view) ────────────
  const presetCount = useMemo(() => {
    const slots = new Set<number>(pendingWhole);
    sel.forEach((k) => slots.add(slotOfKey(k)));
    return slots.size;
  }, [sel, pendingWhole]);
  const sceneCount = sel.size;

  return {
    phase,
    setPhase,
    rows,
    store,
    sel,
    pendingWhole,
    expanded,
    ready,
    presetCount,
    sceneCount,
    sceneInfo,
    footswitchInfo,
    ampCandidates: lib.ampCandidates,
    blocksByIndex: lib.blocksByIndex,
    /** Per-preset signal graph from the startup backup, keyed by 0-based list index —
     *  the source for each row's real CPU readout. */
    graphByIndex: lib.graphByIndex,
    scan,
    togglePreset,
    toggleKey,
    toggleExpand,
    clearSelection,
    deselectKeys,
    toggleAll,
    profileByName,
    targetLufsByName,
    refresh,
  };
}
