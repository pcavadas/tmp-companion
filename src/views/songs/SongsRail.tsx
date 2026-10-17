// src/views/songs/SongsRail.tsx — the Songs tab's sidebar contents (look B; `SplitView`
// draws the column): the Setlists | Presets switch, "All songs" with the song count,
// then either the unit's setlists (each with its song count, the list scrolling on its
// own, and "New setlist" pinned at the foot) or its presets (read-only, from the
// startup backup scan, each with how many songs use it).

import type { CSSProperties } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { RailItem, RailLabel } from "../../ui/Rail";
import { Button, SegmentedControl } from "../../ui/primitives";
import { InlineEdit } from "../../ui/InlineEdit";
import { DASH } from "../../lib/format";
import type { SetlistRecord, SongRecord } from "../../lib/types";
import type { LibraryScan } from "../../device/libraryScan";
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
  // The setlists (or presets) scroll on their own, under the fixed top and above the
  // pinned foot.
  const list: CSSProperties = {
    flex: 1,
    minHeight: 0,
    overflowY: "auto",
    display: "flex",
    flexDirection: "column",
    gap: t.sp2,
  };
  const caption: CSSProperties = {
    fontSize: t.fontCaption,
    lineHeight: `${String(t.lineCaption)}px`,
    color: t.textSecondary,
    padding: `${String(t.sp4)}px ${String(t.sp8)}px`,
  };
  const label: CSSProperties = {
    padding: `${String(t.sp8 + 2)}px ${String(t.sp8)}px ${String(t.sp2)}px`,
  };

  return (
    <>
      <div style={{ marginBottom: t.sp8 }}>
        <SegmentedControl
          value={axis}
          ariaLabel="Browse by"
          size="sm"
          onChange={onAxis}
          options={[
            { value: "setlists", label: "Setlists" },
            { value: "presets", label: "Presets" },
          ]}
        />
      </div>
      <RailItem
        label="All songs"
        meta={songCount}
        active={view.mode === "all"}
        onClick={() => {
          onView({ mode: "all" });
        }}
      />
      {axis === "setlists" ? (
        <>
          <RailLabel style={label}>Setlists</RailLabel>
          <div style={list}>
            {setlists.map((l) => (
              <RailItem
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
              <div style={caption}>No setlists yet.</div>
            )}
          </div>
          <div style={{ marginTop: "auto", paddingTop: t.sp8 }}>
            {creatingList ? (
              <InlineEdit
                initial=""
                placeholder="Setlist name"
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
                  fontFamily: t.fontSans,
                  fontSize: t.fontBody,
                }}
                wrapStyle={{
                  display: "flex",
                  alignItems: "center",
                  gap: t.sp6,
                  height: t.control,
                  boxSizing: "border-box",
                  border: `1px solid ${t.controlBorder}`,
                  borderRadius: t.radiusControl,
                  padding: `0 ${String(t.sp6)}px 0 ${String(t.sp8)}px`,
                  background: t.surface,
                }}
                commitLabel="Create setlist"
              />
            ) : (
              <Button
                variant="plain"
                icon="plus"
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
          <RailLabel style={label}>Presets on your Tone Master Pro</RailLabel>
          <div style={list}>
            {!lib.ready || lib.presets.length === 0 ? (
              <div style={caption}>
                {lib.ready
                  ? "No presets on your Tone Master Pro."
                  : "Reading presets…"}
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
                    onView({ mode: "preset", slot: p.slot });
                  }}
                />
              ))
            )}
          </div>
        </>
      )}
    </>
  );
}
