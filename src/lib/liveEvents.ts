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
// Each wrapper is a thin typed `listen<T>(name, cb)` over @tauri-apps/api/event,
// mirroring App.tsx's existing `listen("tmp://device-detached", …)` pattern. They
// return the `Promise<UnlistenFn>` Tauri gives back; callers store it and call the
// resolved fn on cleanup. Inert (no-op unlisten) outside Tauri so Vitest/jsdom and a
// plain `vite` browser session don't touch the (absent) event bridge.

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

/** Event names the monitor emits (must match `monitor::EVT_*` in the backend). */
export const LIVE_EVENT = {
  livePreset: "tmp://live-preset",
  liveScene: "tmp://live-scene",
  sceneList: "tmp://scene-list",
  signalChain: "tmp://signal-chain",
  sync: "tmp://sync",
  backupProgress: "tmp://backup-progress",
  levelingLufs: "tmp://leveling-lufs",
} as const;

/** Async no-op unlisten for the off-Tauri path (browser / Vitest). */
const NOOP_UNLISTEN: Promise<UnlistenFn> = Promise.resolve(() => {
  /* no-op */
});

/** Subscribe via `listen` only inside Tauri; off-Tauri (browser / Vitest) return
 * the inert no-op unlisten so the absent event bridge is never touched. The
 * `subscribe` thunk carries the payload type through `listen`'s own generic. */
function guardedListen(
  subscribe: () => Promise<UnlistenFn>,
): Promise<UnlistenFn> {
  return isTauri() ? subscribe() : NOOP_UNLISTEN;
}

/** `tmp://live-preset` — active preset identity (coalesced name + list index + flags). */
export const onLivePreset = (
  cb: (e: LivePresetEvent) => void,
): Promise<UnlistenFn> =>
  guardedListen(() =>
    listen<LivePresetEvent>(LIVE_EVENT.livePreset, (e) => {
      cb(e.payload);
    }),
  );

/** `tmp://live-scene` — the scene the unit just recalled (base vs FS classified). */
export const onLiveScene = (
  cb: (e: LiveSceneEvent) => void,
): Promise<UnlistenFn> =>
  guardedListen(() =>
    listen<LiveSceneEvent>(LIVE_EVENT.liveScene, (e) => {
      cb(e.payload);
    }),
  );

/** `tmp://scene-list` — the active preset's live scene names (pushed on preset load). */
export const onSceneList = (
  cb: (e: SceneListEvent) => void,
): Promise<UnlistenFn> =>
  guardedListen(() =>
    listen<SceneListEvent>(LIVE_EVENT.sceneList, (e) => {
      cb(e.payload);
    }),
  );

/** `tmp://signal-chain` — the active preset's signal graph (same shape as ActiveGraph). */
export const onSignalChain = (
  cb: (e: SignalChainEvent) => void,
): Promise<UnlistenFn> =>
  guardedListen(() =>
    listen<SignalChainEvent>(LIVE_EVENT.signalChain, (e) => {
      cb(e.payload);
    }),
  );

/** `tmp://sync` — a device-push / (re)connect is in flight (neutral catching-up state). */
export const onSync = (cb: (e: SyncEvent) => void): Promise<UnlistenFn> =>
  guardedListen(() =>
    listen<SyncEvent>(LIVE_EVENT.sync, (e) => {
      cb(e.payload);
    }),
  );

/** `tmp://backup-progress` — emitted by `read_library_via_backup` as the device
 * streams its library backup; drives the Presets-tab two-phase scan strip. (Not a
 * monitor event, but the same typed-listen plumbing applies.) */
export const onBackupProgress = (
  cb: (e: BackupProgress) => void,
): Promise<UnlistenFn> =>
  guardedListen(() =>
    listen<BackupProgress>(LIVE_EVENT.backupProgress, (e) => {
      cb(e.payload);
    }),
  );

/** `tmp://leveling-lufs` — advisory live measured loudness streamed (~5×/sec) while a
 * leveling capture runs; drives the run row's "measuring…" readout. Reference-level,
 * NOT the final value — the result row is the confirm. (Not a monitor event, but the
 * same typed-listen plumbing applies.) */
export const onLevelingLufs = (
  cb: (e: LiveLufsEvent) => void,
): Promise<UnlistenFn> =>
  guardedListen(() =>
    listen<LiveLufsEvent>(LIVE_EVENT.levelingLufs, (e) => {
      cb(e.payload);
    }),
  );
