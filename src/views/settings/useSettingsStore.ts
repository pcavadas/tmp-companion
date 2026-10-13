// src/views/settings/useSettingsStore.ts — the Settings tab's persisted data: the
// store (profiles, loudness targets, playback level) and the pickup topologies.
//
// Load and save are never silent. The store and the pickup topologies load together
// into a loading | error | ready state: nothing that edits a list is offered until it
// is ready, because save_targets / save_profiles replace the WHOLE list (and
// save_profiles unlinks the captures of every "removed" profile), so an add against
// the initial empty arrays would overwrite the stored list. A failed load is retried
// with `retryLoad`; a failed save sets `saveError` and re-reads the store, so the page
// shows what is actually stored. Every save is optimistic: the list updates at once.

import { useEffect, useRef, useState } from "react";

import {
  getStore,
  listPickupTopologies,
  saveProfiles,
  saveTargets,
  setPlaybackLevel,
} from "../../lib/invoke";
import { errMsg } from "../../lib/format";
import type {
  PlaybackLevel,
  Profile,
  Store,
  Target,
  TopologyInfo,
} from "../../lib/types";

// The wire target is name-only ({name, lufs}); the UI carries a transient `uid`
// so React keys, drag-reorder, and the auto-rename of a freshly added row stay
// stable across renames (names aren't unique) and persist (uid is stripped).
export interface UiTarget extends Target {
  uid: string;
}
const stripUid = (rows: UiTarget[]): Target[] =>
  rows.map(({ name, lufs }) => ({ name, lufs }));

/** The initial read of the store + topologies. */
export type LoadState =
  { kind: "loading" } | { kind: "error"; message: string } | { kind: "ready" };

export interface SettingsStore {
  load: LoadState;
  ready: boolean;
  retryLoad: () => void;
  /** The last failed save, shown until the next save succeeds. */
  saveError: string | null;
  profiles: Profile[];
  targets: UiTarget[];
  playback: PlaybackLevel;
  topologies: TopologyInfo[];
  /** A fresh transient target uid. Call it from an event handler, never in render. */
  newUid: () => string;
  /** Replace the target list locally, without persisting (a slider drag move). */
  setTargets: (update: (prev: UiTarget[]) => UiTarget[]) => void;
  persistTargets: (next: UiTarget[]) => Promise<void>;
  persistProfiles: (next: Profile[]) => Promise<void>;
  persistPlayback: (level: PlaybackLevel) => Promise<void>;
  /** Re-read the profiles after a calibration (its result is already saved). */
  refreshFromStore: () => Promise<void>;
}

export function useSettingsStore(): SettingsStore {
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [targets, setTargets] = useState<UiTarget[]>([]);
  const [playback, setPlayback] = useState<PlaybackLevel>("stage");
  const [topologies, setTopologies] = useState<TopologyInfo[]>([]);
  const [load, setLoad] = useState<LoadState>({ kind: "loading" });
  const [saveError, setSaveError] = useState<string | null>(null);
  // Monotonic counter for transient target uids.
  const uidRef = useRef(0);
  const newUid = () => `t${String(uidRef.current++)}`;
  // Bumped to (re-)read the store: by Retry after a failed load, and after a
  // failed save so the page shows what is actually stored.
  const [readGen, setReadGen] = useState(0);
  // Bumped as each save starts. A recovery read that sees it move discards its
  // result: a newer optimistic edit outranks what was stored before it.
  const saveSeqRef = useRef(0);

  // Read the store and the topologies together; either failing is a load error.
  // A superseded read (a newer readGen, or unmount) applies nothing, and a read
  // overtaken by a newer save leaves that save's optimistic lists in place.
  useEffect(() => {
    let current = true;
    const seq = saveSeqRef.current;
    Promise.all([getStore(), listPickupTopologies()])
      .then(([store, tp]: [Store, TopologyInfo[]]) => {
        if (!current) return;
        if (saveSeqRef.current !== seq) {
          // A save started during this read; its own outcome decides the lists
          // (a failure re-reads again).
          setLoad({ kind: "ready" });
          return;
        }
        setProfiles(store.profiles);
        setTargets(
          // Stamp a transient uid per row (uidRef is stable across renders).
          store.targets.map((row) => ({
            ...row,
            uid: `t${String(uidRef.current++)}`,
          })),
        );
        setPlayback(store.playback_level);
        setTopologies(tp);
        setLoad({ kind: "ready" });
      })
      .catch((e: unknown) => {
        if (current) setLoad({ kind: "error", message: errMsg(e) });
      });
    return () => {
      current = false;
    };
  }, [readGen]);

  function retryLoad() {
    setLoad({ kind: "loading" });
    setReadGen((g) => g + 1);
  }

  // A save failed: say so, and re-read the store so the lists show what is
  // stored rather than the change that never landed.
  function saveFailed(what: string, e: unknown) {
    setSaveError(
      `Couldn't save your ${what}: ${errMsg(e)}. Showing what is saved instead.`,
    );
    setReadGen((g) => g + 1);
  }

  // Optimistic: the list updates at once; a failed save re-reads the store.
  async function persistProfiles(next: Profile[]) {
    setProfiles(next);
    saveSeqRef.current++;
    try {
      await saveProfiles(next);
      setSaveError(null);
    } catch (e) {
      saveFailed("instruments", e);
    }
  }

  // Persist a target list (strips the transient uid for the wire model).
  async function persistTargets(next: UiTarget[]) {
    setTargets(next);
    saveSeqRef.current++;
    try {
      await saveTargets(stripUid(next));
      setSaveError(null);
    } catch (e) {
      saveFailed("loudness targets", e);
    }
  }

  async function persistPlayback(level: PlaybackLevel) {
    setPlayback(level);
    saveSeqRef.current++;
    try {
      await setPlaybackLevel(level);
      setSaveError(null);
    } catch (e) {
      saveFailed("playback level", e);
    }
  }

  // After a real calibration resolves, re-read the store so the persisted
  // calibration_lufs shows on the row. A failed re-read keeps the current rows:
  // the calibration itself already succeeded and was saved by the backend.
  async function refreshFromStore() {
    try {
      const store = await getStore();
      setProfiles(store.profiles);
    } catch {
      // nothing to refresh; the row keeps its current state.
    }
  }

  return {
    load,
    ready: load.kind === "ready",
    retryLoad,
    saveError,
    profiles,
    targets,
    playback,
    topologies,
    newUid,
    setTargets,
    persistTargets,
    persistProfiles,
    persistPlayback,
    refreshFromStore,
  };
}
