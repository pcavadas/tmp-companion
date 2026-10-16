// src/views/songs/skeletons.tsx — the Songs tab while its songs and setlists are read
// (look B). Songs + setlists are the slowest payload from the unit, so the page ghosts
// in place on the same geometry as the real one — the same SplitView sidebar, the same
// title, the same 36-high rows in a Group — and the lists simply fill in when they land.
// The real headings stay so it never reads as broken; one line beside a spinner,
// "Reading songs and setlists…", says it is still working.

import { useTheme } from "../../theme/ThemeContext";
import { sectionLabel } from "../../theme/tokens";
import { SplitView } from "../../ui/SplitView";
import { Group, Row } from "../../ui/List";
import { RailLabel } from "../../ui/Rail";
import { Skel } from "../../ui/Skeleton";
import { Spinner } from "../../ui/Spinner";
import { SongsPage } from "./SongsPage";

// Per-row name widths, so the ghost list reads as varied songs.
const SONG_SKEL_W = [150, 118, 172, 134, 108, 160, 126, 144, 112];
// A few ghost setlists under the real "Setlists" label.
const RAIL_SKEL_W = [118, 92, 134, 78];

export function SongsLoadingSkeleton() {
  const { t } = useTheme();
  const railRow = (w: number, key: number | string, label?: string) => (
    <div
      key={key}
      style={{
        height: t.control,
        flexShrink: 0,
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        padding: `0 ${String(t.sp8)}px`,
        fontSize: t.fontBody,
        color: t.text,
      }}
    >
      {label ?? <Skel w={w} h={10} />}
      <Skel w={12} h={9} />
    </div>
  );
  return (
    <SplitView
      sidebarLabel="Sections"
      sidebar={
        <>
          {/* Where the Setlists | Presets switch lands. */}
          <Skel
            w={132}
            h={t.controlSmall + 2 * t.sp2}
            r={t.radiusPanel}
            style={{ marginBottom: t.sp8 }}
          />
          {railRow(0, "all", "All songs")}
          <RailLabel
            style={{
              padding: `${String(t.sp8 + 2)}px ${String(t.sp8)}px ${String(t.sp2)}px`,
            }}
          >
            Setlists
          </RailLabel>
          {RAIL_SKEL_W.map((w, i) => railRow(w, i))}
        </>
      }
    >
      <SongsPage
        title="All songs"
        subtitle={
          <span
            role="status"
            style={{ display: "inline-flex", alignItems: "center", gap: t.sp6 }}
          >
            <Spinner size={12} strokeWidth={2.2} />
            Reading songs and setlists…
          </span>
        }
      >
        <Group>
          <Row header style={{ height: t.control - 1 }}>
            <span style={{ ...sectionLabel(t), width: 30, flexShrink: 0 }}>
              №
            </span>
            <span style={sectionLabel(t)}>Song</span>
          </Row>
          {SONG_SKEL_W.map((w, i) => (
            <Row key={i}>
              <span style={{ width: 30, flexShrink: 0, display: "flex" }}>
                <Skel w={16} h={9} />
              </span>
              <Skel w={w} h={11} />
            </Row>
          ))}
        </Group>
      </SongsPage>
    </SplitView>
  );
}
