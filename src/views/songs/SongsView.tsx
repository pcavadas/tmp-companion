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
// GLOBAL song slot) is cached per setlist and re-fetched on select; it is cleared
// whenever a song is added/removed (those shift every global slot).
//
// Operations are SERIALIZED (single USB connection) via `runDeviceOp` — a module-
// scoped promise QUEUE: an action taken while another op runs waits its turn, it is
// never dropped. Controls are disabled while busy, failures surface as a Toast (never
// swallowed: the device is truth, so the UI must not show an un-persisted edit).
// Every successful op is applied as ONE reducer to both the view's state and the
// libraryScan snapshot the next mount paints from, so a tab switch never repaints a
// stale setlist list. Slot-keyed setlist writes carry the setlist's expected name;
// the backend refuses a stale slot. Only mounted when connected (App gates the
// disconnected branch).

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
import { shiftMembershipAfterDelete, songBpm } from "./songUtil";
import type { SetlistMember, SongDraft } from "./shared";
import { SongList } from "./SongList";
import { SetlistDetail } from "./SetlistDetail";
import { PresetDetail, UnitBadge } from "./PresetDetail";
import {
  subscribeLibraryScan,
  getLibraryScan,
  invalidateSongPresetJoin,
  patchLibrarySongs,
} from "../level/libraryScan";
import { SongsLoadingSkeleton } from "./skeletons";

// ---------------------------------------------------------------------------
// device-op queue + the songs data reducer
// ---------------------------------------------------------------------------

/** The view's device-backed data. `membership`: setlistSlot → ordered GLOBAL song
 *  slots; an absent key = not read yet. */
interface SongsData {
  songs: SongRecord[];
  setlists: SetlistRecord[];
  membership: Map<number, number[]>;
}

/** The tail of the device-op queue. MODULE-scoped so it outlives a tab switch: a
 *  remount waits for an unmounted instance's queued writes before it paints. */
let opQueue: Promise<void> = Promise.resolve();

/** Apply a successful op's reducer to the libraryScan snapshot (the next mount's
 *  instant-paint seed) — even when this view has since unmounted. */
function patchSnapshot(reduce: (d: SongsData) => SongsData): void {
  const scan = getLibraryScan();
  if (!scan.ready) return;
  const next = reduce({
    songs: scan.songs,
    setlists: scan.setlists,
    membership: scan.setlistSongs,
  });
  patchLibrarySongs({
    songs: next.songs,
    setlists: next.setlists,
    setlistSongs: next.membership,
  });
}

/** A song slot no song resolves to (an out-of-band edit, or an empty entry): shown
 *  as its own row — never filtered out, which would shift every later row onto the
 *  wrong device position. */
const unresolvedSong = (slot: number): SongRecord => ({
  slot,
  name: slot === 0 ? "Empty entry" : "Unknown song",
  notes: "",
  bpm: 0,
  bpm_active: false,
});

// ---------------------------------------------------------------------------
// root
// ---------------------------------------------------------------------------

type View =
  | { mode: "all" }
  | { mode: "setlist"; slot: number }
  | { mode: "preset"; slot: number };
type RailAxis = "setlists" | "presets";
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
  const { phase, mountedRef, runLoad } = useDeviceLoad();
  const [data, setData] = useState<SongsData>(() => ({
    songs: [],
    setlists: [],
    membership: new Map(),
  }));
  const { songs, setlists, membership } = data;
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

  // This instance's queued + running device ops (drives `busy`).
  const pendingRef = useRef(0);
  // Setlist slots with a membership read already queued — a repeat click on the same
  // setlist doesn't stack a second read behind it.
  const pendingReads = useRef(new Set<number>());
  const toastTimer = useRef<number | null>(null);

  const showToast = useCallback(
    (message: string, kind: ToastKind) => {
      setToast({ message, kind });
      if (toastTimer.current) window.clearTimeout(toastTimer.current);
      toastTimer.current = window.setTimeout(() => {
        if (mountedRef.current) setToast(null);
      }, 4200);
    },
    [mountedRef],
  );

  // Clear the pending toast-dismiss timer on unmount. The mountedRef guard already
  // blocks the setState, but don't leave a dangling timer (InstrumentRow's pattern).
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
  // edited in Pro Control, not here). `null` = the join is UNKNOWN (a song add/remove
  // shifted every song slot since the scan) — rendered as "—", never as 0.
  const presetMembers = useMemo(() => {
    const join = lib.songPresetSlots;
    if (join == null) return null;
    const m = new Map<number, SongRecord[]>();
    songs.forEach((s) => {
      (join.get(s.slot) ?? []).forEach((idx) => {
        const arr = m.get(idx);
        if (arr) arr.push(s);
        else m.set(idx, [s]);
      });
    });
    return m;
  }, [songs, lib.songPresetSlots]);

  // Initial / connected-edge load. FAST PATH: a settled startup backup scan already
  // holds songs + setlists + membership (the slowest live payloads — two sequential
  // fail-closed handshakes, ~5–9 s) → paint instantly from it, no device read. Every
  // successful write patches that snapshot (`patchSnapshot`), so a remount paints the
  // post-edit state. COLD OPEN (scan not ready yet during the initial ~22 s backup)
  // falls back to the live read. Live reads (here + listSetlistSongs on select) stay
  // the read-back-after-write source for CRUD. Live reads are SEQUENTIAL, not
  // parallel — each opens its own exclusive HID connection and the TMP is
  // single-connection (concurrent connects collide with kIOReturnExclusiveAccess
  // 0xe00002c5). The load first waits out any write still queued by a previous mount.
  const refresh = useCallback(async () => {
    await runLoad(async () => {
      await opQueue;
      const scan = getLibraryScan();
      const fromScan = scan.ready && scan.songs.length > 0;
      const s = fromScan ? scan.songs : await listSongs();
      const l = fromScan ? scan.setlists : await readSetlists();
      const m = fromScan
        ? new Map(scan.setlistSongs)
        : new Map<number, number[]>();
      if (!fromScan)
        patchSnapshot(() => ({ songs: s, setlists: l, membership: m }));
      if (!mountedRef.current) return;
      setData({ songs: s, setlists: l, membership: m });
      setView((v) =>
        v.mode === "setlist" && !l.some((x) => x.slot === v.slot)
          ? { mode: "all" }
          : v,
      );
    });
  }, [runLoad, mountedRef]);

  useEffect(() => {
    if (connected) void refresh();
  }, [connected, refresh]);

  // Device ops run ONE at a time (single USB connection) on the module-scoped queue:
  // an action taken while another op runs WAITS its turn — it is never dropped. On
  // success `reduce` is applied to this view's data AND to the libraryScan snapshot
  // (even after an unmount), then `after` runs; a failed write applies nothing — the
  // UI must NOT show an un-persisted edit — and surfaces a Toast.
  const runDeviceOp = useCallback(
    <T,>(
      fn: () => Promise<T>,
      reduce: (d: SongsData, r: T) => SongsData,
      after?: (r: T) => void,
    ): Promise<void> => {
      pendingRef.current += 1;
      setBusy(true);
      const run = opQueue.then(async () => {
        try {
          const r = await fn();
          patchSnapshot((d) => reduce(d, r));
          if (mountedRef.current) setData((d) => reduce(d, r));
          after?.(r);
        } catch (e) {
          if (mountedRef.current) showToast(errMsg(e), "err");
        } finally {
          pendingRef.current -= 1;
          if (mountedRef.current && pendingRef.current === 0) setBusy(false);
        }
      });
      opQueue = run;
      return run;
    },
    [showToast, mountedRef],
  );

  // Fetch a setlist's membership (always re-read on select for freshness).
  const loadMembership = useCallback(
    (slot: number) => {
      if (pendingReads.current.has(slot)) return; // already queued — it will land
      pendingReads.current.add(slot);
      void runDeviceOp(
        () =>
          listSetlistSongs(slot).finally(() =>
            pendingReads.current.delete(slot),
          ),
        (d, slots) => ({
          ...d,
          membership: new Map(d.membership).set(slot, slots),
        }),
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
        () => apiCreateSongFull(d.name, d.notes.trim() || null, d.bpm, null),
        (s, out) => ({ ...s, songs: out.songs, membership: new Map() }),
        (out) => {
          invalidateSongPresetJoin(); // slots shifted → backup song↔preset join unknown
          warnBpm(out.bpm_warning);
        },
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
        () => apiUpdateSongFull(rec.slot, name, notes, bpm),
        (s, out) => ({ ...s, songs: out.songs }),
        (out) => {
          warnBpm(out.bpm_warning);
        },
      );
    },
    [runDeviceOp, warnBpm],
  );

  const doDeleteSong = useCallback(
    (slot: number, name: string) => {
      void runDeviceOp(
        () => apiRemoveSong(slot, name),
        // slots shifted — every cached membership is stale
        (s, fresh) => ({ ...s, songs: fresh, membership: new Map() }),
        () => {
          invalidateSongPresetJoin(); // …and the backup song↔preset join is unknown
        },
      );
    },
    [runDeviceOp],
  );

  // ── setlist ops ──
  // Every slot-keyed setlist write carries the setlist's NAME as it is shown: the
  // backend re-reads the list and refuses a slot that now names another setlist.
  const addSetlist = useCallback(
    (name: string) => {
      void runDeviceOp(
        () => apiAddSetlist(name),
        (s, fresh) => ({ ...s, setlists: fresh }),
      );
    },
    [runDeviceOp],
  );

  const renameSetlist = useCallback(
    (slot: number, expectName: string, name: string) => {
      void runDeviceOp(
        () => apiRenameSetlist(slot, expectName, name),
        (s, fresh) => ({ ...s, setlists: fresh }),
      );
    },
    [runDeviceOp],
  );

  // Delete shifts every later setlist down one slot: re-key the membership cache and
  // the selection the same way, or a later row shows (and edits) its neighbour.
  const doDeleteSetlist = useCallback(
    (slot: number, name: string) => {
      void runDeviceOp(
        () => apiRemoveSetlist(slot, name),
        (s, fresh) => ({
          ...s,
          setlists: fresh,
          membership: shiftMembershipAfterDelete(s.membership, slot),
        }),
        () => {
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
    [runDeviceOp],
  );

  // ── membership ops (positions are 1-based within the setlist) ──
  const setMembers =
    (setlistSlot: number) =>
    (s: SongsData, members: number[]): SongsData => ({
      ...s,
      membership: new Map(s.membership).set(setlistSlot, members),
    });

  // Multi-add = ONE batched transaction (add_setlist_songs): N writes under a
  // single bookend with a single final membership read.
  const addSongsToSetlist = useCallback(
    (setlistSlot: number, expectName: string, songSlots: number[]) => {
      void runDeviceOp(
        () => apiAddSetlistSongs(setlistSlot, expectName, songSlots),
        setMembers(setlistSlot),
      );
    },
    [runDeviceOp],
  );

  const removeFromSetlist = useCallback(
    (setlistSlot: number, expectName: string, position: number) => {
      void runDeviceOp(
        () => apiRemoveSetlistSong(setlistSlot, expectName, position),
        setMembers(setlistSlot),
      );
    },
    [runDeviceOp],
  );

  const reorderSetlist = useCallback(
    (
      setlistSlot: number,
      expectName: string,
      oldPos: number,
      newPos: number,
    ) => {
      if (oldPos === newPos) return;
      void runDeviceOp(
        () => apiMoveSetlistSong(setlistSlot, expectName, oldPos, newPos),
        setMembers(setlistSlot),
      );
    },
    [runDeviceOp],
  );

  // Create a song from within a setlist picker and immediately add it — ONE
  // batched transaction (create_song_full with add_to_setlist). The create shifts
  // every song slot, so re-apply songs AND keep only this setlist's fresh
  // membership (others are re-read on select). `members` comes back null only when
  // the backend couldn't resolve the created song by name (duplicate-name edge) —
  // then clear the cache so the membership is re-read on select.
  const createAndAdd = useCallback(
    (setlistSlot: number, d: SongDraft) => {
      void runDeviceOp(
        () =>
          apiCreateSongFull(d.name, d.notes.trim() || null, d.bpm, setlistSlot),
        (s, out) => ({
          ...s,
          songs: out.songs,
          membership:
            out.members != null
              ? new Map([[setlistSlot, out.members]])
              : new Map<number, number[]>(),
        }),
        (out) => {
          invalidateSongPresetJoin(); // create shifted slots → join unknown
          warnBpm(out.bpm_warning);
        },
      );
    },
    [runDeviceOp, warnBpm],
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
  // Every device entry becomes a row carrying its RAW 1-based position — an entry no
  // song resolves to is shown as a placeholder, never filtered out (filtering would
  // send a later row's render index as the device position: a wrong-entry write).
  const currentMembers: SetlistMember[] | null =
    currentMemberSlots === undefined
      ? null
      : currentMemberSlots.map((slot, i) => ({
          position: i + 1,
          song: songsBySlot.get(slot) ?? unresolvedSong(slot),
        }));
  const memberSlotSet = new Set(currentMemberSlots ?? []);
  const availableForSetlist = songs.filter((s) => !memberSlotSet.has(s.slot));

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
            members={currentMembers}
            available={availableForSetlist}
            busy={busy}
            onRename={(n) => {
              renameSetlist(current.slot, current.name, n);
            }}
            onDelete={() => {
              setConfirm({
                kind: "setlist",
                slot: current.slot,
                name: current.name,
              });
            }}
            onRemoveSong={(pos) => {
              removeFromSetlist(current.slot, current.name, pos);
            }}
            onReorder={(o, n) => {
              reorderSetlist(current.slot, current.name, o, n);
            }}
            onAdd={(slots) => {
              addSongsToSetlist(current.slot, current.name, slots);
            }}
            onCreateAndAdd={(d) => {
              createAndAdd(current.slot, d);
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

export default SongsView;
