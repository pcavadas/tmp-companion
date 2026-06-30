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
// Operations are SERIALIZED (single USB connection) via `runDeviceOp` — one in-
// flight op at a time, controls disabled while busy, failures surfaced as a Toast
// (never swallowed: the device is truth, so the UI must not show an un-persisted
// edit). Only mounted when connected (App gates the disconnected branch).

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
import {
  AlertBanner,
  Button,
  Modal,
  SegmentedControl,
  Toast,
} from "../../ui/primitives";
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
import { songBpm } from "./songUtil";
import type { SongDraft } from "./shared";
import { SongList } from "./SongList";
import { SetlistDetail } from "./SetlistDetail";
import { PresetDetail, UnitBadge } from "./PresetDetail";
import {
  subscribeLibraryScan,
  getLibraryScan,
  invalidateLibrarySongs,
} from "../level/libraryScan";
import { SongsLoadingSkeleton } from "./skeletons";

// ---------------------------------------------------------------------------
// left-rail item
// ---------------------------------------------------------------------------
interface SetlistRailItemProps {
  label: string;
  count: number | null;
  active: boolean;
  onClick: () => void;
}

function SetlistRailItem({
  label,
  count,
  active,
  onClick,
}: SetlistRailItemProps) {
  const { t } = useTheme();
  return (
    <div
      onClick={onClick}
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        padding: "8px 10px",
        borderRadius: t.rMd,
        cursor: "pointer",
        background: active ? t.accentSoft : "transparent",
        borderLeft: active ? `2px solid ${t.accent}` : "2px solid transparent",
      }}
    >
      <span
        style={{
          fontFamily: t.serif,
          fontSize: t.fsName2,
          color: active ? t.ink : t.ink2,
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
        }}
      >
        {label}
      </span>
      <span
        style={{
          fontFamily: t.mono,
          fontSize: t.fsData2,
          color: t.faint,
          flexShrink: 0,
          marginLeft: 8,
        }}
      >
        {count ?? DASH}
      </span>
    </div>
  );
}

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

  const busyRef = useRef(false);
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

  const songsBySlot = useMemo(
    () => new Map(songs.map((s) => [s.slot, s])),
    [songs],
  );

  // Presets axis (read-only): preset LIST INDEX → the songs that use it, built once per
  // (songs, bindings) change and reused for BOTH the rail counts and the detail pane —
  // avoids an O(songs × presets) re-filter on every render. `lib.songPresetSlots` is
  // keyed by the live song slot; this is a snapshot join the unit owns (assignments are
  // edited in Pro Control, not here).
  const presetMembers = useMemo(() => {
    const m = new Map<number, SongRecord[]>();
    songs.forEach((s) => {
      (lib.songPresetSlots.get(s.slot) ?? []).forEach((idx) => {
        const arr = m.get(idx);
        if (arr) arr.push(s);
        else m.set(idx, [s]);
      });
    });
    return m;
  }, [songs, lib.songPresetSlots]);

  // Initial / connected-edge load. FAST PATH: a settled startup backup scan already
  // holds songs + setlists + membership (the slowest live payloads — two sequential
  // fail-closed handshakes, ~5–9 s) → paint instantly from it, no device read. COLD
  // OPEN (scan not ready yet during the initial ~22 s backup) falls back to the live
  // read. Live reads (here + listSetlistSongs on select) stay the read-back-after-write
  // source for CRUD. Live reads are SEQUENTIAL, not parallel — each opens its own
  // exclusive HID connection and the TMP is single-connection (concurrent connects
  // collide with kIOReturnExclusiveAccess 0xe00002c5).
  const refresh = useCallback(async () => {
    await runLoad(async () => {
      const scan = getLibraryScan();
      const fromScan = scan.ready && scan.songs.length > 0;
      const s = fromScan ? scan.songs : await listSongs();
      const l = fromScan ? scan.setlists : await readSetlists();
      if (!mountedRef.current) return;
      setSongs(s);
      setSetlists(l);
      setMembership(fromScan ? new Map(scan.setlistSongs) : new Map());
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

  // One in-flight device op at a time (single USB connection). `apply` runs only on
  // success — a failed write must NOT leave the UI showing an un-persisted edit.
  const runDeviceOp = useCallback(
    async <T,>(fn: () => Promise<T>, apply: (r: T) => void): Promise<void> => {
      if (busyRef.current) return;
      busyRef.current = true;
      setBusy(true);
      try {
        const r = await fn();
        if (mountedRef.current) apply(r);
      } catch (e) {
        if (mountedRef.current) showToast(errMsg(e), "err");
      } finally {
        busyRef.current = false;
        if (mountedRef.current) setBusy(false);
      }
    },
    [showToast, mountedRef],
  );

  // Fetch a setlist's membership (always re-read on select for freshness).
  const loadMembership = useCallback(
    (slot: number) => {
      void runDeviceOp(
        () => listSetlistSongs(slot),
        (slots) => {
          setMembership((m) => new Map(m).set(slot, slots));
        },
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
        (out) => {
          setSongs(out.songs);
          setMembership(new Map());
          invalidateLibrarySongs(); // slots shifted → backup song↔preset join is stale
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
        (out) => {
          setSongs(out.songs);
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
        (fresh) => {
          setSongs(fresh);
          setMembership(new Map()); // slots shifted — every cached membership is stale
          invalidateLibrarySongs(); // …and the backup song↔preset join is stale too
        },
      );
    },
    [runDeviceOp],
  );

  // ── setlist ops ──
  const addSetlist = useCallback(
    (name: string) => {
      void runDeviceOp(() => apiAddSetlist(name), setSetlists);
    },
    [runDeviceOp],
  );

  const renameSetlist = useCallback(
    (slot: number, name: string) => {
      void runDeviceOp(() => apiRenameSetlist(slot, name), setSetlists);
    },
    [runDeviceOp],
  );

  const doDeleteSetlist = useCallback(
    (slot: number, name: string) => {
      void runDeviceOp(
        () => apiRemoveSetlist(slot, name),
        (fresh) => {
          setSetlists(fresh);
          setMembership((m) => {
            const n = new Map(m);
            n.delete(slot);
            return n;
          });
          setView((v) =>
            v.mode === "setlist" && v.slot === slot ? { mode: "all" } : v,
          );
        },
      );
    },
    [runDeviceOp],
  );

  // ── membership ops (positions are 1-based within the setlist) ──
  // Multi-add = ONE batched transaction (add_setlist_songs): N writes under a
  // single bookend with a single final membership read.
  const addSongsToSetlist = useCallback(
    (setlistSlot: number, songSlots: number[]) => {
      void runDeviceOp(
        () => apiAddSetlistSongs(setlistSlot, songSlots),
        (members) => {
          setMembership((m) => new Map(m).set(setlistSlot, members));
        },
      );
    },
    [runDeviceOp],
  );

  const removeFromSetlist = useCallback(
    (setlistSlot: number, position: number) => {
      void runDeviceOp(
        () => apiRemoveSetlistSong(setlistSlot, position),
        (members) => {
          setMembership((m) => new Map(m).set(setlistSlot, members));
        },
      );
    },
    [runDeviceOp],
  );

  const reorderSetlist = useCallback(
    (setlistSlot: number, oldPos: number, newPos: number) => {
      if (oldPos === newPos) return;
      void runDeviceOp(
        () => apiMoveSetlistSong(setlistSlot, oldPos, newPos),
        (members) => {
          setMembership((m) => new Map(m).set(setlistSlot, members));
        },
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
        (out) => {
          setSongs(out.songs);
          setMembership(
            out.members != null
              ? new Map([[setlistSlot, out.members]])
              : new Map(),
          );
          invalidateLibrarySongs(); // create shifted slots → stale backup join
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
  const currentMembers: SongRecord[] | null =
    currentMemberSlots === undefined
      ? null
      : currentMemberSlots
          .map((slot) => songsBySlot.get(slot))
          .filter((s): s is SongRecord => Boolean(s));
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
      <div style={{ padding: 28 }}>
        <AlertBanner style={{ marginBottom: 14 }}>{phase.message}</AlertBanner>
        <Button variant="primary" onClick={() => void refresh()}>
          Try again
        </Button>
      </div>
    );
  }

  // Shared rail styles (the setlists + presets sections share their section label,
  // scroll container, and empty-hint markup).
  const railLabel: CSSProperties = {
    fontFamily: t.mono,
    fontSize: t.fsMicro,
    letterSpacing: t.lsWide,
    color: t.faint,
    textTransform: "uppercase",
  };
  const railScroll: CSSProperties = {
    flex: 1,
    minHeight: 0,
    overflowY: "auto",
    display: "flex",
    flexDirection: "column",
    gap: 2,
    margin: "0 -2px",
    padding: "0 2px",
  };
  const railHint: CSSProperties = {
    fontFamily: t.sans,
    fontSize: t.fsLabel,
    color: t.faint,
    padding: "4px 10px",
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
        {/* rail */}
        <div
          style={{
            borderRight: `0.5px solid ${t.hairline}`,
            background: t.bgAlt,
            padding: "12px 10px",
            display: "flex",
            flexDirection: "column",
            gap: 2,
            minHeight: 0,
          }}
        >
          <div style={{ padding: "2px 2px 10px" }}>
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
          <SetlistRailItem
            label="All songs"
            count={songs.length}
            active={view.mode === "all"}
            onClick={() => {
              setView({ mode: "all" });
            }}
          />
          {railAxis === "setlists" ? (
            <>
              <div style={{ ...railLabel, padding: "16px 8px 8px" }}>
                Setlists
              </div>
              <div style={railScroll}>
                {setlists.map((l) => (
                  <SetlistRailItem
                    key={l.slot}
                    label={l.name}
                    count={membership.get(l.slot)?.length ?? null}
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
              <div style={{ paddingTop: 8 }}>
                {creatingList ? (
                  <div
                    style={{
                      display: "flex",
                      alignItems: "center",
                      gap: 6,
                      border: `0.5px solid ${t.accent}`,
                      borderRadius: t.rMd,
                      padding: "4px 5px 4px 9px",
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
                  padding: "16px 8px 8px",
                }}
              >
                <span style={railLabel}>Presets</span>
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
                    <SetlistRailItem
                      key={p.slot}
                      label={p.name}
                      count={presetMembers.get(p.slot)?.length ?? 0}
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
        </div>

        {/* detail */}
        {view.mode === "preset" && currentPreset ? (
          <PresetDetail
            preset={currentPreset}
            members={presetMembers.get(currentPreset.slot) ?? []}
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
              renameSetlist(current.slot, n);
            }}
            onDelete={() => {
              setConfirm({
                kind: "setlist",
                slot: current.slot,
                name: current.name,
              });
            }}
            onRemoveSong={(pos) => {
              removeFromSetlist(current.slot, pos);
            }}
            onReorder={(o, n) => {
              reorderSetlist(current.slot, o, n);
            }}
            onAdd={(slots) => {
              addSongsToSetlist(current.slot, slots);
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
