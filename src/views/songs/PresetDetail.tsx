// src/views/songs/PresetDetail.tsx — the Presets-axis detail: a READ-ONLY list of the
// songs that use one preset ("songs per preset"). Which songs use a preset is set ON
// THE UNIT (Pro Control); this view only displays it, sourced from the startup backup
// scan. No add / remove / edit controls — reinforced with an "on unit" lock badge + a
// footer note.

import type { CSSProperties } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { slotLabel } from "../../lib/format";
import type { SongRecord } from "../../lib/types";
import { songBpm } from "./songUtil";

// Single-line ellipsis — shared by the title, song name, and notes.
const truncate: CSSProperties = {
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
};
// Shared grid for the column header + each song row (height/borders differ).
const rowGrid: CSSProperties = {
  display: "grid",
  gridTemplateColumns: "34px 1fr 78px",
  alignItems: "center",
  padding: "0 18px",
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
        <div
          style={{
            flex: 1,
            minHeight: 0,
            borderTop: `0.5px solid ${t.hairline}`,
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            justifyContent: "center",
            gap: 13,
            padding: "0 44px",
            textAlign: "center",
          }}
        >
          <span
            style={{
              width: 46,
              height: 46,
              borderRadius: 12,
              border: `0.5px solid ${t.hairlineStrong}`,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
            }}
          >
            <Icon name="wave" size={20} stroke={t.faint} />
          </span>
          <div style={{ fontFamily: t.serif, fontSize: 18, color: t.ink2 }}>
            No songs use this preset
          </div>
          <div
            style={{
              fontFamily: t.sans,
              fontSize: 12.5,
              color: t.mutedInk,
              maxWidth: 340,
              lineHeight: 1.55,
            }}
          >
            Which songs reach for a preset is set on the unit in Pro Control.
            This view stays in sync with the device.
          </div>
        </div>
      ) : (
        <>
          <div
            style={{
              ...rowGrid,
              height: 28,
              borderTop: `0.5px solid ${t.hairline}`,
              borderBottom: `0.5px solid ${t.hairline}`,
              fontFamily: t.mono,
              fontSize: t.fsMicro,
              letterSpacing: "0.1em",
              textTransform: "uppercase",
              color: t.faint,
            }}
          >
            <span>№</span>
            <span>song</span>
            <span style={{ textAlign: "right" }}>bpm</span>
          </div>
          <div style={{ flex: 1, minHeight: 0, overflowY: "auto" }}>
            {members.map((s, i) => {
              const bpm = songBpm(s);
              return (
                <div
                  key={s.slot}
                  style={{
                    ...rowGrid,
                    height: 48,
                    borderBottom: `0.5px solid ${t.hairline}`,
                  }}
                >
                  <span
                    style={{
                      fontFamily: t.mono,
                      fontSize: 11,
                      color: t.mutedInk,
                    }}
                  >
                    {String(i + 1).padStart(2, "0")}
                  </span>
                  <div style={{ minWidth: 0, paddingRight: 12 }}>
                    <div
                      style={{
                        fontFamily: t.serif,
                        fontSize: 14.5,
                        color: t.ink,
                        ...truncate,
                      }}
                    >
                      {s.name}
                    </div>
                    {s.notes && (
                      <div
                        style={{
                          marginTop: 1,
                          fontFamily: t.sans,
                          fontSize: 11.5,
                          color: t.mutedInk,
                          ...truncate,
                        }}
                      >
                        {s.notes}
                      </div>
                    )}
                  </div>
                  <span
                    style={{
                      fontFamily: t.mono,
                      fontSize: 11.5,
                      textAlign: "right",
                      color: bpm != null ? t.ink2 : t.faint,
                    }}
                  >
                    {bpm != null ? `${String(bpm)} bpm` : "—"}
                  </span>
                </div>
              );
            })}
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
