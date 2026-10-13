// src/views/songs/SongsRail.tsx — the Songs tab's left rail (the DS Rail, shared with
// the Settings category sidebar): the Setlists | Presets axis switch, "All songs", then
// either the unit's setlists (+ "New setlist") or its presets (read-only, from the
// startup backup scan), each with its song count.

import type { CSSProperties } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Rail, RailItem, RailLabel } from "../../ui/Rail";
import { Button, SegmentedControl } from "../../ui/primitives";
import { InlineEdit } from "../../ui/InlineEdit";
import { DASH } from "../../lib/format";
import type { SetlistRecord, SongRecord } from "../../lib/types";
import type { LibraryScan } from "../../device/libraryScan";
import { UnitBadge } from "./UnitBadge";
import type { SongsPane } from "./useSongsDevice";

export type RailAxis = "setlists" | "presets";

export interface SongsRailProps {
  axis: RailAxis;
  /** Switch axis (the caller also returns the pane to "All songs"). */
  onAxis: (axis: RailAxis) => void;
  view: SongsPane;
  onView: (view: SongsPane) => void;
  songCount: number;
  setlists: SetlistRecord[];
  /** setlistSlot → its GLOBAL song slots; absent = not read yet (shows a dash). */
  membership: Map<number, number[]>;
  onSelectSetlist: (slot: number) => void;
  lib: LibraryScan;
  /** preset list index → the songs that use it; null = the join is unknown. */
  presetMembers: Map<number, SongRecord[]> | null;
  busy: boolean;
  creatingList: boolean;
  onCreatingList: (open: boolean) => void;
  onCommitList: (name: string) => void;
}

export function SongsRail({
  axis,
  onAxis,
  view,
  onView,
  songCount,
  setlists,
  membership,
  onSelectSetlist,
  lib,
  presetMembers,
  busy,
  creatingList,
  onCreatingList,
  onCommitList,
}: SongsRailProps) {
  const { t } = useTheme();
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
    <Rail style={{ gap: t.space1 }}>
      <div
        style={{
          padding: `${String(t.space1)}px ${String(t.space1)}px ${String(t.space5)}px`,
        }}
      >
        <SegmentedControl
          variant="light"
          value={axis}
          ariaLabel="Browse by"
          onChange={onAxis}
          options={[
            { value: "setlists", label: "Setlists", icon: "list" },
            { value: "presets", label: "Presets", icon: "wave" },
          ]}
        />
      </div>
      <RailItem
        nav="current"
        label="All songs"
        meta={songCount}
        active={view.mode === "all"}
        onClick={() => {
          onView({ mode: "all" });
        }}
      />
      {axis === "setlists" ? (
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
                nav="current"
                key={l.slot}
                label={l.name}
                meta={membership.get(l.slot)?.length ?? DASH}
                active={view.mode === "setlist" && view.slot === l.slot}
                onClick={() => {
                  onSelectSetlist(l.slot);
                }}
              />
            ))}
            {setlists.length === 0 && (
              <div style={railHint}>No setlists on the unit.</div>
            )}
          </div>
          <div style={{ paddingTop: t.space4 }}>
            {creatingList ? (
              <InlineEdit
                initial=""
                placeholder="Name this setlist"
                // The ✓ / ✕ beside the field are its click path, so a blur
                // (e.g. onto ✕) must not create the setlist.
                commitOnBlur={false}
                onCommit={onCommitList}
                onCancel={() => {
                  onCreatingList(false);
                }}
                style={{
                  flex: 1,
                  minWidth: 0,
                  fontFamily: t.serif,
                  fontSize: t.fsBody2,
                }}
                wrapStyle={{
                  display: "flex",
                  alignItems: "center",
                  gap: t.space3,
                  border: `0.5px solid ${t.accent}`,
                  borderRadius: t.rMd,
                  padding: `${String(t.space2)}px ${String(t.space3)}px ${String(t.space2)}px ${String(t.space4)}px`,
                  background: t.bg,
                }}
                commitLabel="Create"
              />
            ) : (
              <Button
                variant="ghost"
                icon="plus"
                size="sm"
                disabled={busy}
                onClick={() => {
                  onCreatingList(true);
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
                  nav="current"
                  key={p.slot}
                  label={p.name}
                  meta={
                    presetMembers
                      ? (presetMembers.get(p.slot)?.length ?? 0)
                      : DASH
                  }
                  active={view.mode === "preset" && view.slot === p.slot}
                  onClick={() => {
                    onView({ mode: "preset", slot: p.slot });
                  }}
                />
              ))
            )}
          </div>
        </>
      )}
    </Rail>
  );
}
