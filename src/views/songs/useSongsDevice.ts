// src/views/songs/useSongsDevice.ts — the Songs tab's device-backed state and its op
// queue: the songs, setlists and per-setlist membership read back from the unit, the
// shown view, the FIFO of device ops (`runDeviceOp`, one op on the wire at a time),
// every song / setlist / membership write, and the toast that surfaces a failure.
// See SongsView's header for the read-back-after-write and slot-shift rules these
// callbacks implement. Toasts say what happened in plain words, chosen by kind (a
// failed write or read, a taken name, a tempo that didn't land); the backend's own
// text goes to the log, never on screen.

import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";

import type { ToastKind } from "../../ui/primitives";
import {
  listSongs,
  readSetlists,
  listSetlistSongs,
  createSongFull as apiCreateSongFull,
  updateSongFull as apiUpdateSongFull,
  removeSong as apiRemoveSong,
  addSetlist as apiAddSetlist,
  renameSetlist as apiRenameSetlist,
  removeSetlist as apiRemoveSetlist,
  addSetlistSongs as apiAddSetlistSongs,
  removeSetlistSong as apiRemoveSetlistSong,
  moveSetlistSong as apiMoveSetlistSong,
  logUnlessCommandError,
} from "../../lib/invoke";
import { logError } from "../../lib/log";
import { useDeviceLoad } from "../../lib/useDeviceLoad";
import type {
  SongRecord,
  SetlistRecord,
  SongSaveOutcome,
} from "../../lib/types";
import {
  songBpm,
  shiftMembershipAfterDelete,
  slotAfterDeletes,
} from "./songUtil";
import type { SetlistEntry, SongDraft } from "./songUtil";
import {
  subscribeLibraryScan,
  getLibraryScan,
  invalidateSongPresetJoin,
  patchLibrarySongs,
  forgetLibrarySongs,
} from "../../device/libraryScan";

/** What the detail pane shows: every song, one setlist, or one preset's songs. */
export type SongsPane =
  | { mode: "all" }
  | { mode: "setlist"; slot: number }
  | { mode: "preset"; slot: number };
type DeviceOpEffect = "read" | "write" | "shiftsSongs";

/** A write the unit refused or that failed on the wire. */
export const WRITE_FAILED =
  "Couldn't save that change to your Tone Master Pro.";
/** A setlist's songs couldn't be read (the one read that runs on the op queue). */
export const READ_FAILED =
  "Couldn't read this setlist from your Tone Master Pro.";
/** The song saved but its tempo didn't land (the warning's text goes to the log). */
export const TEMPO_FAILED = "Song saved, but its tempo didn't. Set it again.";
/** A create whose name another song already had (the unit keeps both). */
export const nameTaken = (name: string) =>
  `You already have a song called ${name}.`;

export function useSongsDevice(connected: boolean) {
  const { phase, runLoad } = useDeviceLoad();
  // Whether this view is still mounted — NOT a setState guard (a no-op on React
  // 19), but runDeviceOp's branch: a write that finishes after the tab unmounted
  // must stale the scan snapshot instead of applying to a view that's gone.
  const mountedRef = useRef(true);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);
  const [songs, setSongs] = useState<SongRecord[]>([]);
  const [setlists, setSetlists] = useState<SetlistRecord[]>([]);
  // setlistSlot → ordered GLOBAL song slots (membership). Absent = not yet read.
  const [membership, setMembership] = useState<Map<number, number[]>>(
    new Map(),
  );
  const [view, setView] = useState<SongsPane>({ mode: "all" });
  const [busy, setBusy] = useState(false);
  // The Presets axis reads the unit's presets + each song's preset bindings from the
  // ONE startup backup scan (App drives it on connect) — no live device read here.
  const lib = useSyncExternalStore(subscribeLibraryScan, getLibraryScan);
  const [toast, setToast] = useState<{
    message: string;
    kind: ToastKind;
  } | null>(null);

  // FIFO of device ops: each op chains onto the previous one, so an action taken
  // while another runs is queued, never dropped. `pending` counts queued + running.
  const queueRef = useRef<Promise<void>>(Promise.resolve());
  const pendingRef = useRef(0);
  // Every setlist delete this view applied, in order: a read queued before one of them
  // re-addresses its setlist through them when it runs.
  const deletedSetlistsRef = useRef<number[]>([]);
  // True once this mount has painted the unit's lists — from then on the view's state
  // is what gets written back into the scan snapshot.
  const [loaded, setLoaded] = useState(false);
  const toastTimer = useRef<number | null>(null);

  const showToast = useCallback((message: string, kind: ToastKind) => {
    setToast({ message, kind });
    if (toastTimer.current) window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => {
      setToast(null);
    }, 4200);
  }, []);

  // Clear the pending toast-dismiss timer on unmount — don't leave a dangling
  // timer (InstrumentRow's pattern).
  useEffect(
    () => () => {
      if (toastTimer.current) window.clearTimeout(toastTimer.current);
    },
    [],
  );

  const songsBySlot = useMemo(
    () => new Map(songs.map((s) => [s.slot, s])),
    [songs],
  );

  // Presets axis (read-only): preset LIST INDEX → the songs that use it, built once per
  // (songs, bindings) change and reused for BOTH the rail counts and the detail pane —
  // avoids an O(songs × presets) re-filter on every render. `lib.songPresetSlots` is
  // keyed by the live song slot; this is a snapshot join the unit owns (assignments are
  // edited in Pro Control, not here).
  // `null` = the join is UNKNOWN (a song add/remove shifted the slots since the scan).
  const songPresetSlots = lib.songPresetSlots;
  const presetMembers = useMemo(() => {
    if (songPresetSlots == null) return null;
    const m = new Map<number, SongRecord[]>();
    songs.forEach((s) => {
      (songPresetSlots.get(s.slot) ?? []).forEach((idx) => {
        const arr = m.get(idx);
        if (arr) arr.push(s);
        else m.set(idx, [s]);
      });
    });
    return m;
  }, [songs, songPresetSlots]);

  // Initial / connected-edge load. FAST PATH: a settled startup backup scan already
  // holds songs + setlists + membership (the slowest live payloads — two sequential
  // fail-closed handshakes, ~5–9 s) → paint instantly from it, no device read. COLD
  // OPEN (scan not ready yet during the initial ~22 s backup) falls back to the live
  // read. Live reads (here + listSetlistSongs on select) stay the read-back-after-write
  // source for CRUD. Live reads are SEQUENTIAL, not parallel — each opens its own
  // exclusive HID connection and the TMP is single-connection (concurrent connects
  // collide with kIOReturnExclusiveAccess 0xe00002c5).
  const refresh = useCallback(async () => {
    await runLoad(async (isCurrent) => {
      const scan = getLibraryScan();
      const fromScan = scan.ready && scan.songs.length > 0;
      const s = fromScan ? scan.songs : await listSongs();
      if (!isCurrent()) return;
      const l = fromScan ? scan.setlists : await readSetlists();
      if (!isCurrent()) return;
      setSongs(s);
      setSetlists(l);
      setMembership(fromScan ? new Map(scan.setlistSongs) : new Map());
      setLoaded(true);
      setView((v) =>
        v.mode === "setlist" && !l.some((x) => x.slot === v.slot)
          ? { mode: "all" }
          : v,
      );
    });
  }, [runLoad]);

  useEffect(() => {
    if (connected) void refresh();
  }, [connected, refresh]);

  // Write the post-edit lists back into the scan snapshot: tabs unmount on switch,
  // and the next mount paints from the snapshot — without this it would repaint the
  // startup scan (a renamed setlist back under its old name, a deleted one back).
  useEffect(() => {
    if (loaded)
      patchLibrarySongs({ songs, setlists, setlistSongs: membership });
  }, [loaded, songs, setlists, membership]);

  // Device ops run one at a time (single USB connection) on a FIFO queue: an op
  // started while another runs waits for it. `apply` runs only on success — a
  // failed write must NOT leave the UI showing an un-persisted edit. `effect` says what
  // the op can change on the unit: nothing (a read), the Songs lists, or the song slots
  // themselves (a song create or delete, which also stales the song↔preset join).
  const runDeviceOp = useCallback(
    <T>(
      fn: () => Promise<T>,
      apply: (r: T) => void,
      effect: DeviceOpEffect = "write",
    ): Promise<void> => {
      pendingRef.current += 1;
      setBusy(true);
      const run = async () => {
        try {
          const r = await fn();
          // Left the tab while this ran: the unit changed but no view can apply the
          // reply, so make the next visit read live instead of the stale snapshot.
          if (mountedRef.current) apply(r);
          else if (effect !== "read") {
            forgetLibrarySongs();
            if (effect === "shiftsSongs") invalidateSongPresetJoin();
          }
        } catch (e) {
          // A command's failure is already in the log (the invoke wrapper); this
          // catches anything else that threw around it.
          logUnlessCommandError("songs device op", e);
          if (mountedRef.current)
            showToast(effect === "read" ? READ_FAILED : WRITE_FAILED, "err");
        } finally {
          pendingRef.current -= 1;
          if (pendingRef.current === 0) setBusy(false);
        }
      };
      const next = queueRef.current.then(run);
      queueRef.current = next;
      return next;
    },
    [showToast],
  );

  // Fetch a setlist's membership (always re-read on select for freshness). A read
  // queued behind a setlist delete follows the setlist the user picked: the delete
  // shifts every later slot down (or removes this one) before the read runs.
  const loadMembership = useCallback(
    (slot: number) => {
      const mark = deletedSetlistsRef.current.length;
      let at: number | null = slot;
      void runDeviceOp(
        (): Promise<number[] | null> => {
          at = slotAfterDeletes(slot, deletedSetlistsRef.current.slice(mark));
          return at == null ? Promise.resolve(null) : listSetlistSongs(at);
        },
        (slots) => {
          const key = at;
          if (key != null && slots != null)
            setMembership((m) => new Map(m).set(key, slots));
        },
        "read",
      );
    },
    [runDeviceOp],
  );

  const selectSetlist = useCallback(
    (slot: number) => {
      setView({ mode: "setlist", slot });
      loadMembership(slot);
    },
    [loadMembership],
  );

  // What a saved song still needs to say, by kind: a create on a name another song
  // already had (its notes, tempo and setlist were skipped), or a tempo that didn't land
  // (BPM is best-effort on the unit — the active-song tap tempo can fail to settle — so
  // the batched transactions keep the saved song and report it instead).
  const reportSave = useCallback(
    (out: SongSaveOutcome, name: string) => {
      if (out.bpm_warning)
        logError(`song tempo didn't save: ${out.bpm_warning}`);
      if (out.duplicate_name) showToast(nameTaken(name), "warn");
      else if (out.bpm_warning) showToast(TEMPO_FAILED, "warn");
    },
    [showToast],
  );

  // ── song ops ──
  // Create = ONE batched device transaction (create_song_full): add + notes + BPM
  // under a single seize bookend with a single final read (the granular per-field
  // chain paid a bookend + a strict full read per field). The backend resolves the
  // created record BY NAME (a new song inserts at protocol slot 1, shifting all
  // others +1). Membership cache cleared — every global song slot just shifted.
  const addSong = useCallback(
    (d: SongDraft) => {
      void runDeviceOp(
        () =>
          apiCreateSongFull(d.name, d.notes.trim() || null, d.bpm, null, []),
        (out) => {
          setSongs(out.songs);
          setMembership(new Map());
          invalidateSongPresetJoin(); // slots shifted → backup song↔preset join is stale
          reportSave(out, d.name);
        },
        "shiftsSongs",
      );
    },
    [runDeviceOp, reportSave],
  );

  // Edit = ONE batched transaction over the CHANGED fields only (null =
  // unchanged); nothing changed → no device op at all.
  const editSong = useCallback(
    (rec: SongRecord, d: SongDraft) => {
      const name = d.name !== rec.name ? d.name : null;
      const notes = d.notes !== rec.notes ? d.notes : null;
      const bpm = d.bpm != null && d.bpm !== songBpm(rec) ? d.bpm : null;
      if (name == null && notes == null && bpm == null) return;
      void runDeviceOp(
        () => apiUpdateSongFull(rec.slot, rec.name, songs, name, notes, bpm),
        (out) => {
          setSongs(out.songs);
          reportSave(out, d.name);
        },
      );
    },
    [runDeviceOp, reportSave, songs],
  );

  const doDeleteSong = useCallback(
    (slot: number, name: string) => {
      void runDeviceOp(
        () => apiRemoveSong(slot, name, songs),
        (fresh) => {
          setSongs(fresh);
          setMembership(new Map()); // slots shifted — every cached membership is stale
          invalidateSongPresetJoin(); // …and the backup song↔preset join is stale too
        },
        "shiftsSongs",
      );
    },
    [runDeviceOp, songs],
  );

  // ── setlist ops ──
  const addSetlist = useCallback(
    (name: string) => {
      void runDeviceOp(() => apiAddSetlist(name), setSetlists);
    },
    [runDeviceOp],
  );

  const renameSetlist = useCallback(
    (list: SetlistRecord, name: string) => {
      void runDeviceOp(
        () => apiRenameSetlist(list.slot, list.name, setlists, name),
        setSetlists,
      );
    },
    [runDeviceOp, setlists],
  );

  const doDeleteSetlist = useCallback(
    (slot: number, name: string) => {
      void runDeviceOp(
        () => apiRemoveSetlist(slot, name, setlists),
        (fresh) => {
          deletedSetlistsRef.current.push(slot);
          setSetlists(fresh);
          // Every later setlist shifted down one slot: re-key the cache and the view
          // with it, or a row would show (and edit) its neighbour's songs.
          setMembership((m) => shiftMembershipAfterDelete(m, slot));
          setView((v) =>
            v.mode !== "setlist" || v.slot < slot
              ? v
              : v.slot === slot
                ? { mode: "all" }
                : { mode: "setlist", slot: v.slot - 1 },
          );
        },
      );
    },
    [runDeviceOp, setlists],
  );

  // ── membership ops (positions are 1-based within the setlist) ──
  // Multi-add = ONE batched transaction (add_setlist_songs): N writes under a
  // single bookend with a single final membership read.
  const addSongsToSetlist = useCallback(
    (list: SetlistRecord, picked: SongRecord[]) => {
      void runDeviceOp(
        () => apiAddSetlistSongs(list, setlists, picked, songs),
        (members) => {
          setMembership((m) => new Map(m).set(list.slot, members));
        },
      );
    },
    [runDeviceOp, setlists, songs],
  );

  // Remove/move address the entry's DEVICE position, and carry the song slot found
  // there so the backend refuses if the setlist changed underneath.
  const removeFromSetlist = useCallback(
    (list: SetlistRecord, entry: SetlistEntry) => {
      void runDeviceOp(
        () =>
          apiRemoveSetlistSong(
            list.slot,
            list.name,
            setlists,
            entry.position,
            entry.songSlot,
          ),
        (members) => {
          setMembership((m) => new Map(m).set(list.slot, members));
        },
      );
    },
    [runDeviceOp, setlists],
  );

  const reorderSetlist = useCallback(
    (list: SetlistRecord, from: SetlistEntry, to: SetlistEntry) => {
      if (from.position === to.position) return;
      void runDeviceOp(
        () =>
          apiMoveSetlistSong(
            list.slot,
            list.name,
            setlists,
            from.position,
            to.position,
            from.songSlot,
          ),
        (members) => {
          setMembership((m) => new Map(m).set(list.slot, members));
        },
      );
    },
    [runDeviceOp, setlists],
  );

  // Create a song from within a setlist picker and immediately add it — ONE
  // batched transaction (create_song_full with add_to_setlist). The create shifts
  // every song slot, so re-apply songs AND keep only this setlist's fresh
  // membership (others are re-read on select). `members` comes back null only when
  // the backend couldn't resolve the created song by name (`duplicate_name`) — then
  // clear the cache so the membership is re-read on select.
  const createAndAdd = useCallback(
    (list: SetlistRecord, d: SongDraft) => {
      void runDeviceOp(
        () =>
          apiCreateSongFull(
            d.name,
            d.notes.trim() || null,
            d.bpm,
            list,
            setlists,
          ),
        (out) => {
          setSongs(out.songs);
          setMembership(
            out.members != null
              ? new Map([[list.slot, out.members]])
              : new Map(),
          );
          invalidateSongPresetJoin(); // create shifted slots → stale backup join
          reportSave(out, d.name);
        },
        "shiftsSongs",
      );
    },
    [runDeviceOp, reportSave, setlists],
  );

  return {
    phase,
    refresh,
    songs,
    setlists,
    membership,
    songsBySlot,
    presetMembers,
    lib,
    view,
    setView,
    busy,
    toast,
    dismissToast: () => {
      setToast(null);
    },
    selectSetlist,
    addSong,
    editSong,
    doDeleteSong,
    addSetlist,
    renameSetlist,
    doDeleteSetlist,
    addSongsToSetlist,
    removeFromSetlist,
    reorderSetlist,
    createAndAdd,
  };
}
