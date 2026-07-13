// src/views/doctor/SceneConsistency.tsx — the scene-loudness finding as a synthetic
// expandable row inside a preset group (after the sound rows). Collapsed it reads as
// a severity dot + "Level jumps" + one chip ("{worst} ±x.x dB vs base"); expanded it
// shows the plain-language sentence, per-sound delta bars centered on a 0 dB line,
// and the (advisory-only) scene-trim prescription. The backend feeds footswitch
// sounds into this table too, so its rows may carry no wire scene — the delta-bar
// render is data-driven and copes.

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { DiagnosisChip } from "./DiagnosisChip";
import { PrescriptionCard } from "./PrescriptionCard";
import { SevDot } from "./SoundRow";
import { sceneConsistencySev, sevTone } from "./severity";
import type { DoctorSceneConsistency } from "../../lib/types";

/** Signed one-decimal dB with the real minus glyph (e.g. +6 → "+6.0", −3 → "−3.0"). */
function signedDb(db: number): string {
  return `${db < 0 ? "−" : "+"}${Math.abs(db).toFixed(1)}`;
}

export interface SceneConsistencyProps {
  sc: DoctorSceneConsistency;
  listIndex: number;
  presetName: string;
  open: boolean;
  onToggle: () => void;
}

export function SceneConsistency({
  sc,
  listIndex,
  presetName,
  open,
  onToggle,
}: SceneConsistencyProps) {
  const { t } = useTheme();
  const sev = sceneConsistencySev(sc);
  const tone = sevTone(t, sev);
  const maxAbs = Math.max(0, ...sc.rows.map((r) => Math.abs(r.deltaDb)));

  return (
    <div style={{ borderTop: `0.5px solid ${t.hairline}` }}>
      <div
        onClick={onToggle}
        style={{
          display: "flex",
          alignItems: "center",
          gap: t.space5,
          minHeight: 38,
          padding: `${String(t.space3)}px ${String(t.space4)}px ${String(t.space3)}px ${String(t.space3)}px`,
          cursor: "pointer",
          background: open ? t.rowSel : "transparent",
        }}
      >
        <SevDot sev={sev} />
        <span
          style={{
            fontFamily: t.serif,
            fontSize: 14,
            color: t.ink,
            whiteSpace: "nowrap",
            minWidth: 96,
            flexShrink: 0,
          }}
        >
          Level jumps
        </span>
        <span
          style={{
            flex: 1,
            minWidth: 0,
            display: "flex",
            alignItems: "center",
            gap: t.space3,
            overflow: "hidden",
          }}
        >
          <DiagnosisChip
            label={`${sc.worstName} ${signedDb(sc.worstDeltaDb)} dB vs base`}
            sev={sev}
          />
        </span>
        <span
          style={{
            width: 14,
            flexShrink: 0,
            display: "inline-flex",
            justifyContent: "center",
            opacity: 0.6,
          }}
        >
          <span
            style={{
              display: "inline-flex",
              transform: open ? "rotate(90deg)" : "none",
              transition: "transform 0.12s",
            }}
          >
            <Icon name="chev-right" size={13} stroke={tone.fg} />
          </span>
        </span>
      </div>

      {open && (
        <div
          style={{
            padding: `${String(t.space1)}px ${String(t.space5)}px ${String(t.space7)}px ${String(t.space11)}px`,
          }}
        >
          <div
            style={{
              fontFamily: t.sans,
              fontSize: 12.5,
              color: t.ink2,
              lineHeight: 1.55,
            }}
          >
            Your <strong>{sc.worstName}</strong> scene jumps{" "}
            <strong>{signedDb(sc.worstDeltaDb)} dB</strong> against the rest — a
            big volume leap when you stomp between sounds. Pros keep it to +1–3
            dB and lean on a mid boost to cut through.
          </div>
          <div style={{ marginTop: t.space5 }}>
            {sc.rows.map((r, i) => {
              const barColor = r.isRef
                ? t.mutedInk
                : Math.abs(r.deltaDb) > 3
                  ? t.sevWarn
                  : t.good;
              const widthPct =
                maxAbs > 0 ? (Math.abs(r.deltaDb) / maxAbs) * 46 : 0;
              const neg = r.deltaDb < 0;
              return (
                <div
                  key={`${r.name}-${String(i)}`}
                  style={{
                    display: "grid",
                    gridTemplateColumns: "108px 1fr 58px",
                    alignItems: "center",
                    gap: t.space4,
                    padding: `${String(t.space2)}px 0`,
                  }}
                >
                  <div
                    style={{
                      display: "flex",
                      alignItems: "baseline",
                      gap: t.space3,
                      minWidth: 0,
                    }}
                  >
                    <span
                      style={{
                        fontFamily: t.serif,
                        fontSize: t.fsLabel,
                        color: t.ink2,
                        whiteSpace: "nowrap",
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                      }}
                    >
                      {r.name}
                    </span>
                    {r.tag != null && r.tag !== "" && (
                      <span
                        style={{
                          fontFamily: t.mono,
                          fontSize: t.fsTag,
                          letterSpacing: t.lsTag,
                          color: t.accentDeep,
                          flexShrink: 0,
                        }}
                      >
                        {r.tag}
                      </span>
                    )}
                  </div>
                  <div style={{ position: "relative", height: 14 }}>
                    <div
                      style={{
                        position: "absolute",
                        left: "50%",
                        top: 0,
                        bottom: 0,
                        width: 1,
                        background: t.hairlineStrong,
                      }}
                    />
                    <div
                      style={{
                        position: "absolute",
                        top: 3,
                        bottom: 3,
                        width: `${String(widthPct)}%`,
                        left: neg ? undefined : "50%",
                        right: neg ? "50%" : undefined,
                        background: barColor,
                        borderRadius: 2,
                      }}
                    />
                  </div>
                  <span
                    style={{
                      fontFamily: t.mono,
                      fontSize: t.fsData,
                      textAlign: "right",
                      color: barColor,
                    }}
                  >
                    {r.isRef ? "ref" : signedDb(r.deltaDb)}
                  </span>
                </div>
              );
            })}
          </div>
          <div
            style={{
              display: "flex",
              flexDirection: "column",
              gap: t.space5,
              marginTop: t.space5,
            }}
          >
            {sc.rx.map((rx, i) => (
              <PrescriptionCard
                key={`${rx.kind}-${String(i)}`}
                rx={rx}
                listIndex={listIndex}
                presetName={presetName}
                scene
              />
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

export default SceneConsistency;
