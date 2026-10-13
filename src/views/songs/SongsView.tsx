// src/views/songs/SongsView.tsx — Songs & Setlists manager (the Songs view body).
//
// DEVICE-BACKED: the connected Tone Master Pro is the single source of truth.
//   • A SONG is a name + optional notes + optional BPM (footswitch/scene/preset
//     assignment happens ON THE UNIT — out of scope here).
//   • A SETLIST is a name + an ORDERED list of song references; a song can sit in
//     many setlists at once.
//
// READ-BACK-AFTER-WRITE: device slots are POSITIONAL and shift on every add/remove
// (adding a song inserts at protocol slot 1 and bumps every other song +1), so the
// UI never predicts slots. Each write command returns the fresh authoritative list
// and the view re-renders from it. Membership (a setlist's songs, addressed by
// GLOBAL song slot, index i = device position i + 1) is cached per setlist and
// re-fetched on select; it is cleared whenever a song is added/removed (those shift
// every global slot) and re-keyed when a setlist delete shifts the setlist slots.
// Every slot-keyed write also carries the name the user acted on, and the backend
// refuses it when the slot no longer reads that name.
//
// The post-edit lists are written back into the libraryScan snapshot, so leaving the
// tab (which unmounts this view) and coming back repaints the unit's current lists,
// not the startup scan.
//
// Operations are SERIALIZED (single USB connection) via `runDeviceOp` — a FIFO queue,
// one op on the wire at a time; an action taken while another op runs waits its turn
// instead of being dropped. Failures surface as a Toast (never swallowed: the device
// is truth, so the UI must not show an un-persisted edit). Only mounted when
// connected (App gates the disconnected branch).

import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import type { CSSProperties } from "react";
import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { Rail, RailItem, RailLabel } from "../../ui/Rail";
import { LoadErrorPane } from "../LoadErrorPane";
import { Button, Modal, SegmentedControl, Toast } from "../../ui/primitives";
import type { ToastKind } from "../../ui/primitives";
import { EmptyState, UsbC } from "../EmptyState";
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
} from "../../lib/invoke";
import { DASH, errMsg } from "../../lib/format";
import { useDeviceLoad } from "../../lib/useDeviceLoad";
import type { SongRecord, SetlistRecord } from "../../lib/types";
import { plainInput } from "../../theme/tokens";
import {
  songBpm,
  shiftMembershipAfterDelete,
  slotAfterDeletes,
} from "./songUtil";
import type { SetlistEntry } from "./songUtil";
import type { SongDraft } from "./shared";
import { SongList } from "./SongList";
import { SetlistDetail } from "./SetlistDetail";
import { PresetDetail, UnitBadge } from "./PresetDetail";
import {
  subscribeLibraryScan,
  getLibraryScan,
  invalidateSongPresetJoin,
  patchLibrarySongs,
  forgetLibrarySongs,
} from "../../device/libraryScan";
import { SongsLoadingSkeleton } from "./skeletons";

// ---------------------------------------------------------------------------
// root
// ---------------------------------------------------------------------------

type View =
  | { mode: "all" }
  | { mode: "setlist"; slot: number }
  | { mode: "preset"; slot: number };
type RailAxis = "setlists" | "presets";
type DeviceOpEffect = "read" | "write" | "shiftsSongs";
type Confirm =
  | { kind: "song"; slot: number; name: string }
  | { kind: "setlist"; slot: number; name: string }
  | null;

export interface SongsViewProps {
  connected: boolean;
  onScan?: () => void;
}

export function SongsView({ connected, onScan }: SongsViewProps) {
  const { t } = useTheme();
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
  const [view, setView] = useState<View>({ mode: "all" });
  const [railAxis, setRailAxis] = useState<RailAxis>("setlists");
  const [busy, setBusy] = useState(false);
  // The Presets axis reads the unit's presets + each song's preset bindings from the
  // ONE startup backup scan (App drives it on connect) — no live device read here.
  const lib = useSyncExternalStore(subscribeLibraryScan, getLibraryScan);
  const [creatingList, setCreatingList] = useState(false);
  const [draftList, setDraftList] = useState("");
  const [confirm, setConfirm] = useState<Confirm>(null);
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
    <T,>(
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
          if (mountedRef.current) showToast(errMsg(e), "err");
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

  // BPM is best-effort on the unit (active-song tap tempo, can fail to settle) —
  // the batched transactions keep the saved song and surface a warning instead.
  const warnBpm = useCallback(
    (warning: string | null) => {
      if (warning) showToast(`Saved, but BPM didn't stick: ${warning}`, "warn");
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
          warnBpm(out.bpm_warning);
        },
        "shiftsSongs",
      );
    },
    [runDeviceOp, warnBpm],
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
          warnBpm(out.bpm_warning);
        },
      );
    },
    [runDeviceOp, warnBpm, songs],
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
  // the backend couldn't resolve the created song by name (duplicate-name edge) —
  // then clear the cache so the membership is re-read on select.
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
          warnBpm(out.bpm_warning);
        },
        "shiftsSongs",
      );
    },
    [runDeviceOp, warnBpm, setlists],
  );

  const commitList = () => {
    const n = draftList.trim();
    if (n) addSetlist(n);
    setCreatingList(false);
    setDraftList("");
  };

  const current =
    view.mode === "setlist"
      ? (setlists.find((l) => l.slot === view.slot) ?? null)
      : null;
  const currentMemberSlots = current ? membership.get(current.slot) : undefined;
  // Unknown membership (not read yet) is `null` — never "empty": an empty set would
  // offer every song in the picker, including ones already in the setlist.
  const currentEntries: SetlistEntry[] | null =
    currentMemberSlots === undefined
      ? null
      : currentMemberSlots.map((songSlot, i) => ({
          position: i + 1,
          songSlot,
          song: songsBySlot.get(songSlot) ?? null,
        }));
  const availableForSetlist: SongRecord[] | null =
    currentMemberSlots === undefined
      ? null
      : songs.filter((s) => !currentMemberSlots.includes(s.slot));

  const currentPreset =
    view.mode === "preset"
      ? (lib.presets.find((p) => p.slot === view.slot) ?? null)
      : null;

  // Songs + setlists are the slowest payload from the unit; while they arrive the
  // whole two-column layout ghosts in place (see SongsLoadingSkeleton).
  // Disconnected takes priority over loading — songs/setlists (read from the device) live on the unit.
  if (!connected) {
    return (
      <EmptyState
        title="Songs & setlists live on the unit"
        body={
          <>
            The Tone Master Pro keeps your songs and the setlists that order
            them. Connect over <UsbC /> and power it on to build, reorder and
            assign them.
          </>
        }
        onScan={onScan}
      />
    );
  }

  if (phase.kind === "loading") {
    return <SongsLoadingSkeleton />;
  }

  if (phase.kind === "error") {
    return (
      <LoadErrorPane message={phase.message} onRetry={() => void refresh()} />
    );
  }

  // Shared rail styles (the setlists + presets sections share their scroll
  // container and empty-hint markup; the section label is the DS RailLabel).
  const railScroll: CSSProperties = {
    flex: 1,
    minHeight: 0,
    overflowY: "auto",
    display: "flex",
    flexDirection: "column",
    gap: t.space1,
    margin: `0 -${String(t.space1)}px`,
    padding: `0 ${String(t.space1)}px`,
  };
  const railHint: CSSProperties = {
    fontFamily: t.sans,
    fontSize: t.fsLabel,
    color: t.faint,
    padding: `${String(t.space2)}px ${String(t.space5)}px`,
    lineHeight: 1.5,
  };

  return (
    <div
      style={{
        height: "100%",
        display: "flex",
        flexDirection: "column",
        background: t.bg,
        position: "relative",
      }}
    >
      <div
        style={{
          flex: 1,
          minHeight: 0,
          display: "grid",
          gridTemplateColumns: "210px 1fr",
        }}
      >
        {/* rail — the DS Rail (shared with the Settings category sidebar) */}
        <Rail style={{ gap: t.space1 }}>
          <div
            style={{
              padding: `${String(t.space1)}px ${String(t.space1)}px ${String(t.space5)}px`,
            }}
          >
            <SegmentedControl
              variant="light"
              value={railAxis}
              ariaLabel="Browse by"
              onChange={(v) => {
                setRailAxis(v);
                setView({ mode: "all" });
              }}
              options={[
                { value: "setlists", label: "Setlists", icon: "list" },
                { value: "presets", label: "Presets", icon: "wave" },
              ]}
            />
          </div>
          <RailItem
            label="All songs"
            meta={songs.length}
            active={view.mode === "all"}
            onClick={() => {
              setView({ mode: "all" });
            }}
          />
          {railAxis === "setlists" ? (
            <>
              <RailLabel
                style={{
                  padding: `${String(t.space8)}px ${String(t.space4)}px ${String(t.space4)}px`,
                }}
              >
                Setlists
              </RailLabel>
              <div style={railScroll}>
                {setlists.map((l) => (
                  <RailItem
                    key={l.slot}
                    label={l.name}
                    meta={membership.get(l.slot)?.length ?? DASH}
                    active={view.mode === "setlist" && view.slot === l.slot}
                    onClick={() => {
                      selectSetlist(l.slot);
                    }}
                  />
                ))}
                {setlists.length === 0 && (
                  <div style={railHint}>No setlists on the unit.</div>
                )}
              </div>
              <div style={{ paddingTop: t.space4 }}>
                {creatingList ? (
                  <div
                    style={{
                      display: "flex",
                      alignItems: "center",
                      gap: t.space3,
                      border: `0.5px solid ${t.accent}`,
                      borderRadius: t.rMd,
                      padding: `${String(t.space2)}px ${String(t.space3)}px ${String(t.space2)}px ${String(t.space4)}px`,
                      background: t.bg,
                    }}
                  >
                    <input
                      autoFocus
                      value={draftList}
                      onChange={(e) => {
                        setDraftList(e.target.value);
                      }}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") commitList();
                        if (e.key === "Escape") {
                          setCreatingList(false);
                          setDraftList("");
                        }
                      }}
                      placeholder="Name this setlist"
                      style={plainInput(t, {
                        flex: 1,
                        minWidth: 0,
                        fontFamily: t.serif,
                        fontSize: t.fsBody2,
                      })}
                    />
                    <span
                      onClick={commitList}
                      title="Create"
                      style={{ cursor: "pointer", display: "flex" }}
                    >
                      <Icon name="check" size={14} stroke={t.accentDeep} />
                    </span>
                    <span
                      onClick={() => {
                        setCreatingList(false);
                        setDraftList("");
                      }}
                      role="button"
                      aria-label="Cancel"
                      title="Cancel"
                      style={{ cursor: "pointer", display: "flex" }}
                    >
                      <Icon name="x" size={13} stroke={t.faint} />
                    </span>
                  </div>
                ) : (
                  <Button
                    variant="ghost"
                    icon="plus"
                    small
                    disabled={busy}
                    onClick={() => {
                      setCreatingList(true);
                    }}
                  >
                    New setlist
                  </Button>
                )}
              </div>
            </>
          ) : (
            <>
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "space-between",
                  padding: `${String(t.space8)}px ${String(t.space4)}px ${String(t.space4)}px`,
                }}
              >
                <RailLabel>Presets</RailLabel>
                <UnitBadge />
              </div>
              <div style={railScroll}>
                {!lib.ready || lib.presets.length === 0 ? (
                  <div style={railHint}>
                    {lib.ready
                      ? "No presets on the unit."
                      : "Reading presets from the unit…"}
                  </div>
                ) : (
                  lib.presets.map((p) => (
                    <RailItem
                      key={p.slot}
                      label={p.name}
                      meta={
                        presetMembers
                          ? (presetMembers.get(p.slot)?.length ?? 0)
                          : DASH
                      }
                      active={view.mode === "preset" && view.slot === p.slot}
                      onClick={() => {
                        setView({ mode: "preset", slot: p.slot });
                      }}
                    />
                  ))
                )}
              </div>
            </>
          )}
        </Rail>

        {/* detail */}
        {view.mode === "preset" && currentPreset ? (
          <PresetDetail
            preset={currentPreset}
            members={
              presetMembers
                ? (presetMembers.get(currentPreset.slot) ?? [])
                : null
            }
          />
        ) : view.mode === "all" || !current ? (
          <SongList
            songs={songs}
            busy={busy}
            onAddSong={addSong}
            onEditSong={editSong}
            onDeleteSong={(rec) => {
              setConfirm({ kind: "song", slot: rec.slot, name: rec.name });
            }}
          />
        ) : (
          <SetlistDetail
            setlist={current}
            entries={currentEntries}
            available={availableForSetlist}
            busy={busy}
            onRename={(n) => {
              renameSetlist(current, n);
            }}
            onDelete={() => {
              setConfirm({
                kind: "setlist",
                slot: current.slot,
                name: current.name,
              });
            }}
            onRemoveSong={(entry) => {
              removeFromSetlist(current, entry);
            }}
            onReorder={(from, to) => {
              reorderSetlist(current, from, to);
            }}
            onAdd={(picked) => {
              addSongsToSetlist(current, picked);
            }}
            onCreateAndAdd={(d) => {
              createAndAdd(current, d);
            }}
          />
        )}
      </div>

      <Modal
        open={confirm != null}
        kicker="DESTRUCTIVE · WRITES TO THE TONE MASTER PRO"
        headline={
          confirm?.kind === "song" ? (
            <>
              Delete song <em>{confirm.name}</em>?
            </>
          ) : (
            <>
              Delete setlist <em>{confirm?.name}</em>?
            </>
          )
        }
        body={
          confirm?.kind === "song"
            ? "This removes the song from the unit and from every setlist that contains it. This can't be undone."
            : "This removes the setlist from the unit. The songs themselves are kept. This can't be undone."
        }
        applyLabel="Delete"
        applyVariant="warn"
        onCancel={() => {
          setConfirm(null);
        }}
        onApply={() => {
          const c = confirm;
          setConfirm(null);
          if (c?.kind === "song") doDeleteSong(c.slot, c.name);
          else if (c?.kind === "setlist") doDeleteSetlist(c.slot, c.name);
        }}
      />
      {toast && (
        <Toast
          message={toast.message}
          kind={toast.kind}
          onDismiss={() => {
            setToast(null);
          }}
        />
      )}
    </div>
  );
}
