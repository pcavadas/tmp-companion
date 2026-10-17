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

import { listPresets, getStore, setPresetInstrument } from "../../lib/invoke";
import { useDeviceLoad } from "../../lib/useDeviceLoad";
import { baseKey, childKeys } from "./leveling";
import { presetChoice, presetKeys } from "./presetChoice";
import { subscribeLibraryScan, getLibraryScan } from "../../device/libraryScan";
import type { PresetRow } from "./leveling";
import type {
  PresetEntry,
  Store,
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
 * details stream in via ONE device backup (~27 KiB/s; ~85 s for a 2.2 MiB archive) —
 * `read_library_via_backup`, whose
 * `tmp://backup-progress` events drive the scan strip. `scanning` is true for that
 * window. The determinate transfer % is NOT here: the strip reads it itself
 * (`useLibraryScanPercent`), so a progress tick never re-renders this hook's host. */
export interface ScanState {
  scanning: boolean;
}

// The list's empty-slot markers, as the device reports them: "--" / "Empty" / "—".
export function isEmptyName(name: string): boolean {
  const n = name.trim();
  return n === "" || n === "--" || n === "—" || n.toLowerCase() === "empty";
}

export function toPresetRow(p: PresetEntry): PresetRow {
  return { slot: p.slot, name: p.name, empty: isEmptyName(p.name) };
}

export interface UsePresetDataOptions {
  /** Which footswitch roster THIS caller's own `footswitchInfo` (and hence its
   *  selection mechanics — `childKeys`, `chosenFrom`, whole-preset ticking) is built
   *  from: `"levelable"` (default) is the pre-BUG-1 behaviour — a switch with zero
   *  level_params is invisible to this view's tree entirely, which is what Doctor's
   *  own SELECT list still relies on (a non-levelable FS has nothing to diagnose as
   *  its own sound). `"all"` is Level's BUG-1 fix: the FULL roster, so a non-levelable
   *  switch still gets a row (disabled, with a reason) instead of vanishing.
   *
   *  This is a per-CALLER choice, not a data-layer one, because `footswitchInfo` here
   *  drives BOTH the rendered rows AND their `fswKey` addressing (childKeys/toggle) —
   *  those two must read the SAME array (position-indexed) or a row's checkbox can
   *  silently toggle a DIFFERENT switch's key than the one it renders. Mixing
   *  `"levelable"` rendering with `"all"`-derived keys (or vice versa) reintroduces
   *  that misalignment, so don't thread `allFootswitchInfo` into a `"levelable"`
   *  caller's own tree/selection — only into a reader that doesn't address by
   *  position (e.g. Doctor's damage detector, which reads `allFootswitchInfo`
   *  directly, never through this option). */
  footswitchRoster?: "levelable" | "all";
}

export function usePresetData(
  connected: boolean,
  opts: UsePresetDataOptions = {},
) {
  const { footswitchRoster = "levelable" } = opts;
  const { phase, runLoad } = useDeviceLoad();
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
  // Per-caller roster (see `UsePresetDataOptions.footswitchRoster`'s doc): Doctor
  // takes the default `footswitchesPerIndex` (levelable-only, unchanged from before
  // BUG 1); Level opts into the full `allFootswitchesByIndex` roster so a
  // non-levelable switch still gets a row (disabled) instead of vanishing.
  const footswitchInfo =
    footswitchRoster === "all"
      ? lib.allFootswitchesByIndex
      : lib.footswitchesPerIndex;
  const scan: ScanState = { scanning: lib.scanning };

  // rows + sceneInfo + footswitchInfo (and the selection and scan state the toggles
  // decide on) in refs so the stable toggle callbacks see the latest data. Synced in an
  // effect (after commit) rather than during render — the callbacks only read these when
  // they fire, which is always after a commit.
  const rowsRef = useRef<PresetRow[]>(rows);
  const sceneInfoRef = useRef<Map<number, SceneInfo[]>>(sceneInfo);
  const footswitchInfoRef =
    useRef<Map<number, FootswitchInfo[]>>(footswitchInfo);
  const selRef = useRef<Set<string>>(sel);
  const pendingWholeRef = useRef<Set<number>>(pendingWhole);
  const readyRef = useRef(ready);
  useEffect(() => {
    rowsRef.current = rows;
    sceneInfoRef.current = sceneInfo;
    footswitchInfoRef.current = footswitchInfo;
    selRef.current = sel;
    pendingWholeRef.current = pendingWhole;
    readyRef.current = ready;
  });

  // Case-insensitive lookup by target NAME (the leveling flow carries the display
  // name, e.g. "Crunch"). `null` when no configured target has that name (an empty
  // name, a target deleted in Settings, or the store not loaded yet): there is no
  // fallback loudness, so the run refuses such a sound instead of leveling and saving
  // it at a value no dropdown showed (danger.md, UI values).
  const targetLufsByName = useCallback(
    (name: string | null): number | null =>
      store?.targets.find((tg) => tg.name.toLowerCase() === name?.toLowerCase())
        ?.lufs ?? null,
    [store],
  );

  // Remember the instrument chosen for a preset: saved in the app's own
  // store on this computer, never on the unit, and mirrored into this view's copy of the
  // store so the next Set up starts from it. A failed save keeps the old copy (the command
  // logs it); the choice still applies to this run.
  const rememberInstrument = useCallback((slot: number, id: string) => {
    void setPresetInstrument(slot, id)
      .then(() => {
        setStore((st) =>
          st
            ? { ...st, profile_by_slot: { ...st.profile_by_slot, [slot]: id } }
            : st,
        );
      })
      .catch(() => undefined);
  }, []);

  // ── read-only loads (mount + after connect) ───────────────────────────────
  // The active graph is no longer fetched here — it arrives from the combined
  // handshake (initialGraph) and the live monitor (tmp://signal-chain), so the
  // separate readActivePreset round-trip (which doubled the connect time) is gone.
  const refresh = useCallback(async () => {
    await runLoad(async (isCurrent) => {
      const [list, st] = await Promise.all([listPresets(), getStore()]);
      if (!isCurrent()) return;
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
  }, [runLoad]);

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
  // Tick a WHOLE preset (`presetKeys` says what that stands for): when its scenes are
  // known, toggle all child keys (all-on ⇒ clear, else select all). When the scan
  // finished without them, toggle its main sound alone. While the scan is still reading,
  // the tick waits in pendingWhole — the reconcile above folds it into `sel` once scenes
  // land — and an untick also drops a main sound kept from before a re-read. Reads fresh
  // state from refs so the callbacks stay stable.
  const togglePreset = useCallback((slot: number) => {
    const row = rowsRef.current.find((r) => r.slot === slot);
    if (!row || row.empty) return;
    const keys = presetKeys(
      slot,
      sceneInfoRef.current.get(slot),
      footswitchInfoRef.current.get(slot),
      readyRef.current,
    );
    if (keys === null) {
      const on =
        presetChoice(
          slot,
          null,
          selRef.current,
          pendingWholeRef.current.has(slot),
        ) === "on";
      setPendingWhole((prev) => {
        const next = new Set(prev);
        if (on) next.delete(slot);
        else next.add(slot);
        return next;
      });
      if (on) {
        const base = baseKey(slot);
        setSel((prev) => {
          if (!prev.has(base)) return prev;
          const next = new Set(prev);
          next.delete(base);
          return next;
        });
      }
      return;
    }
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

  // Select-all ("Select all N shown"): acts on the presets the filter SHOWS — the caller
  // passes their slots — and leaves every hidden preset's choice as it is. If every
  // shown preset is fully chosen, unchoose them all; otherwise choose every sound of every
  // shown, non-empty preset (a preset still being read waits whole in pendingWhole).
  // Reads fresh state from the refs so it isn't stale.
  const toggleAll = useCallback((slots: readonly number[]) => {
    const named = new Set(
      rowsRef.current.filter((r) => !r.empty).map((r) => r.slot),
    );
    const shown = slots.filter((s) => named.has(s));
    if (shown.length === 0) return;
    const entries = shown.map((slot) => ({
      slot,
      keys: presetKeys(
        slot,
        sceneInfoRef.current.get(slot),
        footswitchInfoRef.current.get(slot),
        readyRef.current,
      ),
    }));
    const allOn = entries.every(
      (e) =>
        presetChoice(
          e.slot,
          e.keys,
          selRef.current,
          pendingWholeRef.current.has(e.slot),
        ) === "on",
    );
    if (allOn) {
      const shownSet = new Set(shown);
      setSel(
        (prev) => new Set([...prev].filter((k) => !shownSet.has(slotOfKey(k)))),
      );
      setPendingWhole(
        (prev) => new Set([...prev].filter((s) => !shownSet.has(s))),
      );
      return;
    }
    setSel((prev) => {
      const next = new Set(prev);
      entries.forEach((e) => e.keys?.forEach((k) => next.add(k)));
      return next;
    });
    setPendingWhole((prev) => {
      const next = new Set(prev);
      entries.forEach((e) => {
        if (e.keys === null) next.add(e.slot);
      });
      return next;
    });
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
    /** Every footswitch (incl. Other-class-only, no level candidate) — Doctor's damage
     *  detector needs the full roster, not the levelable-only `footswitchInfo` list. */
    allFootswitchInfo: lib.allFootswitchesByIndex,
    ampCandidates: lib.ampCandidates,
    blocksByIndex: lib.blocksByIndex,
    silenceHintByIndex: lib.silenceHintByIndex,
    /** Per-preset signal graph from the startup backup, keyed by 0-based list index —
     *  the source for each row's real CPU readout. */
    graphByIndex: lib.graphByIndex,
    /** Per-preset scene overlays as node overrides — Doctor's per-scene graph source. */
    sceneOverridesByIndex: lib.sceneOverridesByIndex,
    scan,
    togglePreset,
    toggleKey,
    toggleExpand,
    deselectKeys,
    toggleAll,
    targetLufsByName,
    rememberInstrument,
    refresh,
  };
}
