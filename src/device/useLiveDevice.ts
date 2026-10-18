// src/device/useLiveDevice.ts — the live-sync event-subscription hook.
//
// It subscribes to the device monitor's `tmp://` events (the stream is described in
// `lib/liveEvents.ts`). App-initiated and device-pushed updates are ONE stream, so the UI
// shows the monitor's `syncing` flag and waits for the push rather than mutating
// optimistically.
//
// Lifecycle: the app-level `connectDevice()` starts the monitor. This hook is only an
// event subscriber; tab switches must not churn the monitor session.
//
// This hook is PURE TRANSPORT: it normalizes the event stream into a single
// `LiveDeviceState` snapshot. Its consumers (LevelView, DoctorView, CopyView,
// levelRunStore) map that snapshot to their own UI state. Keeping the wiring here means
// no view touches `@tauri-apps/api/event` directly.

import { useEffect, useSyncExternalStore } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";

import { onLive } from "../lib/liveEvents";
import { isTauri } from "../lib/invoke";
import type { ActiveGraph, SceneListRow } from "../lib/types";

/** The active preset's live scene, classified base-vs-FS by the backend. `key` is
 * `"base"` (the preset default) or the FS-scene index; `name` may be null when the
 * unit's scene name didn't decode (the row still renders). */
export interface LiveScene {
  key: "base" | number;
  name: string | null;
}

/** The normalized live-device snapshot the views consume. Every field
 * is the unit's REAL current state (or its honest absence), never a software guess. */
export interface LiveDeviceState {
  /** 0-based My-Presets list index of the active preset (matches a PresetRow.slot),
   *  or null when the active preset isn't a My-Presets slot / nothing has loaded. */
  activeListIndex: number | null;
  /** The active preset's display name (from CurrentPresetInfoChanged). */
  activeName: string;
  isDirty: boolean;
  isFavorite: boolean;
  /** The live scene within the active preset, or null before the first SceneLoaded. */
  liveScene: LiveScene | null;
  /** The active preset's scenes (sceneListResponse). `fs` is null for now (em-dash). */
  scenes: SceneListRow[];
  /** The active preset's signal graph (currentPresetDataChanged), or null. */
  graph: ActiveGraph | null;
  /** A device-push / (re)connect is in flight — show the neutral catching-up state. */
  syncing: boolean;
  /** A non-null nonce that bumps on every live-preset push, so the orchestrator can
   *  distinguish a genuine device push from a re-render (drives the row flash). */
  presetNonce: number;
  /** Bumps on every live-scene push (drives the footswitch-change flash). */
  sceneNonce: number;
}

const INITIAL: LiveDeviceState = {
  activeListIndex: null,
  activeName: "",
  isDirty: false,
  isFavorite: false,
  liveScene: null,
  scenes: [],
  graph: null,
  syncing: false,
  presetNonce: 0,
  sceneNonce: 0,
};

// ── MODULE-SCOPED store ──────────────────────────────────────────────────────
// The live device state is APP-GLOBAL — it describes the unit, not a tab — and the
// monitor only PUSHES on a device CHANGE. So it must NOT live in a component's
// useState: a tab switch unmounts LevelView, and a component-local snapshot would
// reset to INITIAL with no fresh push to refill it, reverting the hero to the stale
// connect-time graph/slot. A module store (the `libraryScan` pattern) persists the
// last-known state across remounts so the hero stays on the unit's current preset.
let liveState: LiveDeviceState = INITIAL;
const subs = new Set<() => void>();
let bridgeStarted = false;
let bridgeUnlisten: Promise<UnlistenFn>[] = [];

function setLive(next: LiveDeviceState): void {
  liveState = next; // new ref so useSyncExternalStore re-renders
  for (const cb of subs) cb();
}

/** Optimistically patch the hero's live graph after our OWN structural edit (Copy
 *  save). A live block edit pushes no `currentPresetDataChanged` (field 3), so the hero
 *  would otherwise stay stale until the monitor reconnects — this paints the edited
 *  graph immediately. The next genuine device push overwrites it, so it self-reconciles. */
export function patchLiveGraph(graph: ActiveGraph): void {
  setLive({ ...liveState, graph });
}

/** Reset live STATE to INITIAL — on disconnect; the app-global bridge stays
 *  subscribed (LevelView doesn't remount on disconnect), so reconnect pushes flow
 *  back in and refill the store. */
function resetLiveState(): void {
  setLive(INITIAL);
}

/** FULL reset incl. tearing down the event bridge — for test isolation only (call
 *  in `beforeEach`, like `resetLibraryScan`). Tests clear the mock listener registry
 *  between cases, so the bridge must be re-armable; prod never tears it down.
 *  Known limit: the unlisten callbacks fire async (fire-and-forget) — fine for test
 *  setup, where nothing runs between this reset and the next mount's re-subscribe. */
export function resetLiveDevice(): void {
  for (const p of bridgeUnlisten)
    void p.then((u) => {
      u();
    });
  bridgeUnlisten = [];
  bridgeStarted = false;
  setLive(INITIAL);
}

// Subscribe to the 5 monitor events ONCE for the app's lifetime (the monitor is
// app-global; tab switches must not churn it). Both app-initiated command results
// AND device pushes arrive through these listeners — one event stream.
function startBridgeOnce(): void {
  if (bridgeStarted || !isTauri()) return; // inert under Vitest/jsdom (no bridge)
  bridgeStarted = true;
  bridgeUnlisten = [
    onLive("livePreset", (e) => {
      setLive({
        ...liveState,
        activeListIndex: e.listIndex,
        activeName: e.name,
        isDirty: e.isDirty,
        isFavorite: e.isFavorite,
        presetNonce: liveState.presetNonce + 1,
      });
    }),
    onLive("liveScene", (e) => {
      setLive({
        ...liveState,
        liveScene: { key: e.key, name: e.name },
        sceneNonce: liveState.sceneNonce + 1,
      });
    }),
    onLive("sceneList", (e) => {
      setLive({ ...liveState, scenes: e.scenes });
    }),
    onLive("signalChain", (g) => {
      setLive({ ...liveState, graph: g });
    }),
    onLive("sync", (e) => {
      setLive({ ...liveState, syncing: e.syncing });
    }),
  ];
}

export function useLiveDevice(connected: boolean): LiveDeviceState {
  // Start the app-global bridge on first use (idempotent).
  useEffect(() => {
    startBridgeOnce();
  }, []);

  const state = useSyncExternalStore(
    (cb) => {
      subs.add(cb);
      return () => subs.delete(cb);
    },
    () => liveState,
  );

  // Reset the snapshot on disconnect (the monitor refills on reconnect). In an
  // effect, not during render — the store is shared, so notifying every subscriber
  // mid-render is unsafe; a frame-late reset is fine (the disconnect banner shows).
  // STATE-only (keeps the bridge): LevelView stays mounted on disconnect, so the
  // bridge can't re-arm via remount.
  useEffect(() => {
    if (!connected) resetLiveState();
  }, [connected]);

  return state;
}
