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
// instead of being dropped. Failures surface as a Toast in plain words (never
// swallowed: the device is truth, so the UI must not show an un-persisted edit; the
// backend's text goes to the log). While no unit is connected it shows the shared
// device page instead.
//
// Layout (look B): a SplitView — the sidebar (./SongsRail) beside one content column
// (./SongsPage, 720 px at most). Deletes are confirmed in a Modal whose backdrop does
// nothing, so only its own buttons close it.
//
// Split: the device state, op queue and CRUD callbacks live in ./useSongsDevice.

import { useState } from "react";
import { LoadErrorPane } from "../LoadErrorPane";
import { Modal, Toast } from "../../ui/primitives";
import { SplitView } from "../../ui/SplitView";
import { DeviceState } from "../DeviceState";
import type { NoUnitState } from "../../lib/connectError";
import type { SongRecord } from "../../lib/types";
import type { SetlistEntry } from "./songUtil";
import { SongList } from "./SongList";
import { SetlistDetail } from "./SetlistDetail";
import { PresetDetail } from "./PresetDetail";
import { SongsRail, type RailAxis } from "./SongsRail";
import { useSongsDevice } from "./useSongsDevice";
import { SongsLoadingSkeleton } from "./skeletons";

// ---------------------------------------------------------------------------
// root
// ---------------------------------------------------------------------------

type Confirm =
  | { kind: "song"; slot: number; name: string }
  | { kind: "setlist"; slot: number; name: string }
  | null;

export interface SongsViewProps {
  connected: boolean;
  /** What to show while no unit is connected (App's connect state). Default "off". */
  deviceState?: NoUnitState;
}

export function SongsView({ connected, deviceState = "off" }: SongsViewProps) {
  const {
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
    dismissToast,
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
  } = useSongsDevice(connected);
  const [railAxis, setRailAxis] = useState<RailAxis>("setlists");
  const [creatingList, setCreatingList] = useState(false);
  const [confirm, setConfirm] = useState<Confirm>(null);

  const commitList = (name: string) => {
    if (name) addSetlist(name);
    setCreatingList(false);
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
  if (!connected) return <DeviceState state={deviceState} />;

  if (phase.kind === "loading") {
    return <SongsLoadingSkeleton />;
  }

  if (phase.kind === "error") {
    return <LoadErrorPane what="songs" onRetry={() => void refresh()} />;
  }

  return (
    <div
      style={{
        height: "100%",
        display: "flex",
        flexDirection: "column",
        position: "relative",
      }}
    >
      <SplitView
        sidebarLabel="Sections"
        sidebar={
          <SongsRail
            axis={railAxis}
            onAxis={(v) => {
              setRailAxis(v);
              setView({ mode: "all" });
            }}
            view={view}
            onView={setView}
            songCount={songs.length}
            setlists={setlists}
            membership={membership}
            onSelectSetlist={selectSetlist}
            lib={lib}
            presetMembers={presetMembers}
            busy={busy}
            creatingList={creatingList}
            onCreatingList={setCreatingList}
            onCommitList={commitList}
          />
        }
      >
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
      </SplitView>

      <Modal
        open={confirm != null}
        label={`Delete ${confirm?.name ?? ""}?`}
        headline={`Delete ${confirm?.name ?? ""}?`}
        body={
          confirm?.kind === "song"
            ? "It's removed from your Tone Master Pro and from every setlist that has it. This can't be undone here."
            : "It's removed from your Tone Master Pro. Its songs stay in All songs."
        }
        applyLabel={confirm?.kind === "song" ? "Delete song" : "Delete setlist"}
        applyVariant="danger"
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
          onDismiss={dismissToast}
        />
      )}
    </div>
  );
}
