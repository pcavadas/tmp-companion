// src/views/songs/PresetDetail.tsx — the Presets-axis detail: a READ-ONLY list of the
// songs that use one preset ("songs per preset"). Which songs use a preset is set ON
// THE UNIT (Pro Control); this view only displays it, sourced from the startup backup
// scan. No add / remove / edit controls — reinforced with an "on unit" lock badge + a
// footer note.

import type { CSSProperties } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { PaneEmpty } from "../../ui/PaneEmpty";
import { slotLabel } from "../../lib/format";
import type { SongRecord } from "../../lib/types";
import { SongRow } from "./SongRow";
import { ListHeader } from "./ListHeader";

// The Presets-axis song table shares SongList's grid columns for the read-only rows.
const PRESET_COLS = "34px 1fr 78px";

// Single-line ellipsis — shared by the title, song name, and notes.
const truncate: CSSProperties = {
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
};

/** "on unit" lock badge — presets (and their song assignments) live on the device. */
export function UnitBadge({ note = "on unit" }: { note?: string }) {
  const { t } = useTheme();
  return (
    <span
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 3,
        fontFamily: t.mono,
        fontSize: 8.5,
        letterSpacing: "0.08em",
        textTransform: "uppercase",
        color: t.faint,
      }}
    >
      <Icon name="lock" size={10} stroke={t.faint} />
      {note}
    </span>
  );
}

export interface PresetDetailProps {
  /** `slot` is the 0-based My-Presets list index; `slotLabel` renders it as `058`. */
  preset: { slot: number; name: string };
  /** The songs that use this preset (read-only). */
  members: SongRecord[];
}

export function PresetDetail({ preset, members }: PresetDetailProps) {
  const { t } = useTheme();
  const n = members.length;
  return (
    <div style={{ minHeight: 0, display: "flex", flexDirection: "column" }}>
      <div style={{ padding: "16px 18px 13px" }}>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            fontFamily: t.mono,
            fontSize: t.fsMicro,
            letterSpacing: "0.14em",
            textTransform: "uppercase",
            color: t.accentDeep,
          }}
        >
          <span>Preset · slot {slotLabel(preset.slot)}</span>
          <UnitBadge />
        </div>
        <div
          style={{
            marginTop: 4,
            fontFamily: t.serif,
            fontSize: 24,
            color: t.ink,
            ...truncate,
          }}
        >
          {preset.name}
        </div>
        <div
          style={{
            marginTop: 5,
            fontFamily: t.mono,
            fontSize: 10.5,
            color: t.mutedInk,
          }}
        >
          {n} song{n === 1 ? "" : "s"} use{n === 1 ? "s" : ""} this preset
        </div>
      </div>

      {n === 0 ? (
        <PaneEmpty
          icon="wave"
          title="No songs use this preset"
          body="Which songs reach for a preset is set on the unit in Pro Control. This view stays in sync with the device."
        />
      ) : (
        <>
          <ListHeader
            cols={PRESET_COLS}
            cells={[
              { label: "№" },
              { label: "song" },
              { label: "bpm", align: "right" },
            ]}
          />
          <div style={{ flex: 1, minHeight: 0, overflowY: "auto" }}>
            {members.map((s, i) => (
              <SongRow
                key={s.slot}
                song={s}
                idx={i}
                gridCols={PRESET_COLS}
                bpmAlign="right"
                bpmSuffix
              />
            ))}
          </div>
          <div
            style={{
              flexShrink: 0,
              height: 38,
              display: "flex",
              alignItems: "center",
              padding: "0 18px",
              borderTop: `0.5px solid ${t.hairline}`,
              background: t.bgAlt,
              fontFamily: t.mono,
              fontSize: 10,
              color: t.faint,
            }}
          >
            Song ↔ preset assignments are managed on the unit
          </div>
        </>
      )}
    </div>
  );
}

export default PresetDetail;
