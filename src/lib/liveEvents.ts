// TMP Companion — typed listeners for the live-sync monitor events.
//
// The backend's persistent device monitor (src-tauri/src/monitor.rs) holds the
// device with a dense ~250 ms heartbeat so the unit PUSHES its state changes
// (footswitch taps, scene recalls, preset changes done ON THE HARDWARE) unsolicited.
// It mirrors them as the Tauri events below. App-initiated commands (loadScene /
// loadPreset) route through the device-op gate, which pauses the monitor for the
// command's duration; the resulting device state then returns through the SAME event
// stream — so app-initiated and device-pushed updates are one unified stream.
//
// ONE typed subscriber, `onLive(key, cb)`, over `LiveEventMap` (registry key →
// payload type): the compiler checks every name↔payload pairing, and `LIVE_EVENT`
// must name exactly the map's keys (`satisfies`). It returns the
// `Promise<UnlistenFn>` Tauri gives back; callers store it and call the resolved fn
// on cleanup. Inert (no-op unlisten) outside Tauri so Vitest/jsdom and a plain
// `vite` browser session don't touch the (absent) event bridge.

import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import { isTauri } from "./invoke";
import type {
  BackupProgress,
  LiveLufsEvent,
  LivePresetEvent,
  LiveSceneEvent,
  SceneListEvent,
  SignalChainEvent,
  SyncEvent,
} from "./types";

/** Registry key → the payload each event carries (mirrors in `types.ts`). */
export interface LiveEventMap {
  /** `tmp://live-preset` — active preset identity (coalesced name + list index + flags). */
  livePreset: LivePresetEvent;
  /** `tmp://live-scene` — the scene the unit just recalled (base vs FS classified). */
  liveScene: LiveSceneEvent;
  /** `tmp://scene-list` — the active preset's live scene names (pushed on preset load). */
  sceneList: SceneListEvent;
  /** `tmp://signal-chain` — the active preset's signal graph (same shape as ActiveGraph). */
  signalChain: SignalChainEvent;
  /** `tmp://sync` — a device-push / (re)connect is in flight (neutral catching-up state). */
  sync: SyncEvent;
  /** `tmp://backup-progress` — emitted by `read_library_via_backup` as the device
   * streams its library backup; drives the Presets-tab two-phase scan strip. (Not a
   * monitor event, but the same typed-listen plumbing applies.) */
  backupProgress: BackupProgress;
  /** `tmp://leveling-lufs` — advisory live measured loudness streamed (~5×/sec) while a
   * leveling capture runs; drives the run row's "measuring…" readout. Reference-level,
   * NOT the final value — the result row is the confirm. (Not a monitor event.) */
  levelingLufs: LiveLufsEvent;
}

export type LiveEventKey = keyof LiveEventMap;

/** Event names the monitor emits (must match `monitor::EVT_*` in the backend). */
export const LIVE_EVENT = {
  livePreset: "tmp://live-preset",
  liveScene: "tmp://live-scene",
  sceneList: "tmp://scene-list",
  signalChain: "tmp://signal-chain",
  sync: "tmp://sync",
  backupProgress: "tmp://backup-progress",
  levelingLufs: "tmp://leveling-lufs",
} as const satisfies Record<LiveEventKey, `tmp://${string}`>;

/** Async no-op unlisten for the off-Tauri path (browser / Vitest). */
const NOOP_UNLISTEN: Promise<UnlistenFn> = Promise.resolve(() => {
  /* no-op */
});

/** Subscribe to one live event by its registry key; `cb` gets that event's typed
 * payload. Off-Tauri it returns the inert no-op unlisten. */
export function onLive<K extends LiveEventKey>(
  key: K,
  cb: (payload: LiveEventMap[K]) => void,
): Promise<UnlistenFn> {
  if (!isTauri()) return NOOP_UNLISTEN;
  return listen<LiveEventMap[K]>(LIVE_EVENT[key], (e) => {
    cb(e.payload);
  });
}
