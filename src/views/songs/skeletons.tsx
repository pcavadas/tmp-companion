// src/views/songs/skeletons.tsx — loading skeletons for the Songs view. The songs+
// setlists payload is the slowest to arrive from the unit, so both columns ghost in
// place on the SAME grids as the real rows (zero layout shift on resolve). Built
// against the SHIPPED 4-column library grid (SONG_COLS = № · song · bpm · actions) —
// membership chips live in the setlist detail, not the library, so there is no "in
// setlists" column to ghost.
import type { CSSProperties } from "react";
import { useTheme, useStyles } from "../../theme/ThemeContext";
import { Skel, SkelStatus } from "../../ui/Skeleton";
import { ListHeader } from "./ListHeader";
import { SONG_COLS } from "./shared";

// Per-row name/notes-bar widths, so the ghost library reads as varied songs.
const LIBRARY_SKEL = [
  { name: 150, note: 96 },
  { name: 118, note: 0 },
  { name: 172, note: 120 },
  { name: 134, note: 0 },
  { name: 108, note: 84 },
  { name: 160, note: 0 },
  { name: 126, note: 104 },
  { name: 144, note: 0 },
  { name: 112, note: 76 },
  { name: 156, note: 92 },
];

function LibraryRowsSkeleton({ rows = 9 }: { rows?: number }) {
  const { t } = useTheme();
  return (
    <div>
      {LIBRARY_SKEL.slice(0, rows).map((sk, i) => (
        <div
          key={i}
          style={{
            display: "grid",
            gridTemplateColumns: SONG_COLS,
            alignItems: "center",
            height: 48,
            padding: `0 ${String(t.space8)}px 0 ${String(t.space8)}px`,
            borderBottom: `0.5px solid ${t.hairline}`,
          }}
        >
          <Skel w={16} h={9} />
          <div
            style={{
              paddingRight: t.space6,
              display: "flex",
              flexDirection: "column",
              gap: t.space3,
            }}
          >
            <Skel w={sk.name} h={11} />
            {sk.note > 0 && <Skel w={sk.note} h={8} />}
          </div>
          <Skel w={24} h={9} />
          <span />
        </div>
      ))}
    </div>
  );
}

// A few ghost rows under the real "Setlists" heading: a name bar + a count bar.
const RAIL_SKEL_W = [118, 92, 134, 78, 106];

function RailRowsSkeleton({ rows = 4 }: { rows?: number }) {
  const { t } = useTheme();
  return (
    <>
      {RAIL_SKEL_W.slice(0, rows).map((w, i) => (
        <div
          key={i}
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            padding: `${String(t.space4)}px ${String(t.space5)}px`,
            borderLeft: "2px solid transparent",
          }}
        >
          <Skel w={w} h={11} />
          <Skel w={12} h={9} />
        </div>
      ))}
    </>
  );
}

// Songs + setlists are the slowest payload from the unit. While they load, both
// columns ghost in place (same two-column layout, same grids), so when the lists
// land the placeholders simply fill — no jump. The real headings stay so it never
// reads as broken; the mono caption marks it as work-in-progress.
export function SongsLoadingSkeleton() {
  const { t } = useTheme();
  const s = useStyles();
  const railLbl: CSSProperties = {
    fontFamily: t.mono,
    fontSize: t.fsMicro,
    letterSpacing: t.lsWide,
    color: t.faint,
    textTransform: "uppercase",
  };
  return (
    <div
      style={{
        height: "100%",
        display: "flex",
        flexDirection: "column",
        background: t.bg,
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
            padding: `${String(t.space6)}px ${String(t.space5)}px`,
            display: "flex",
            flexDirection: "column",
            gap: t.space1,
            minHeight: 0,
          }}
        >
          <div
            style={{
              ...railLbl,
              padding: `${String(t.space2)}px ${String(t.space4)}px ${String(t.space4)}px`,
            }}
          >
            Library
          </div>
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
              padding: `${String(t.space4)}px ${String(t.space5)}px`,
              borderLeft: "2px solid transparent",
            }}
          >
            <span
              style={{
                fontFamily: t.serif,
                fontSize: t.fsName2,
                color: t.ink2,
              }}
            >
              All songs
            </span>
            <Skel w={12} h={9} />
          </div>
          <div
            style={{
              ...railLbl,
              padding: `${String(t.space8)}px ${String(t.space4)}px ${String(t.space4)}px`,
            }}
          >
            Setlists
          </div>
          <RailRowsSkeleton rows={4} />
        </div>
        {/* detail (library) */}
        <div style={{ minHeight: 0, display: "flex", flexDirection: "column" }}>
          <div
            style={{
              padding: `${String(t.space8)}px ${String(t.space8)}px ${String(t.space6)}px`,
            }}
          >
            <div style={s.kicker(t.accentDeep)}>Library</div>
            <div
              style={{
                fontFamily: t.serif,
                fontSize: t.fsTitle,
                color: t.ink,
                marginTop: t.space2,
              }}
            >
              All songs
            </div>
            <div style={{ marginTop: t.space4 }}>
              <SkelStatus label="Reading songs & setlists…" />
            </div>
          </div>
          <ListHeader
            cols={SONG_COLS}
            cells={[
              { label: "№" },
              { label: "song" },
              { label: "bpm" },
              { label: "" },
            ]}
          />
          <div style={{ flex: 1, minHeight: 0, overflow: "hidden" }}>
            <LibraryRowsSkeleton rows={9} />
          </div>
        </div>
      </div>
    </div>
  );
}
