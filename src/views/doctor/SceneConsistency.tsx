// src/views/doctor/SceneConsistency.tsx — the per-preset scene-loudness section:
// a plain-language sentence, per-scene delta bars centered on a 0 dB line, and the
// (advisory-only) scene-trim prescription. Shown inside a preset card when the run
// found a loudness jump between scenes.

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { PrescriptionCard } from "./PrescriptionCard";
import type { DoctorSceneConsistency } from "../../lib/types";

/** Signed one-decimal dB with the real minus glyph (e.g. +6 → "+6.0", −3 → "−3.0"). */
function signedDb(db: number): string {
  return `${db < 0 ? "−" : "+"}${Math.abs(db).toFixed(1)}`;
}

export interface SceneConsistencyProps {
  sc: DoctorSceneConsistency;
  listIndex: number;
  presetName: string;
}

export function SceneConsistency({
  sc,
  listIndex,
  presetName,
}: SceneConsistencyProps) {
  const { t } = useTheme();
  const maxAbs = Math.max(0, ...sc.rows.map((r) => Math.abs(r.deltaDb)));

  return (
    <div
      style={{
        marginTop: 10,
        paddingTop: 10,
        borderTop: `0.5px solid ${t.hairline}`,
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
        <Icon name="sliders" size={14} stroke={t.sevWarn} />
        <span
          style={{ fontFamily: t.sans, fontSize: t.fsName2, color: t.sevWarn }}
        >
          Scene consistency
        </span>
      </div>
      <div
        style={{
          fontFamily: t.sans,
          fontSize: t.fsBody,
          color: t.ink2,
          marginTop: 6,
          lineHeight: 1.55,
        }}
      >
        Your <strong>{sc.worstName}</strong> scene jumps{" "}
        <strong>{signedDb(sc.worstDeltaDb)} dB</strong> against the rest — a big
        volume leap when you stomp between sounds. Pros keep it to +1–3 dB and
        lean on a mid boost to cut through.
      </div>
      <div style={{ marginTop: 10 }}>
        {sc.rows.map((r, i) => {
          const barColor = r.isRef
            ? t.mutedInk
            : Math.abs(r.deltaDb) > 3
              ? t.sevWarn
              : t.good;
          const widthPct = maxAbs > 0 ? (Math.abs(r.deltaDb) / maxAbs) * 46 : 0;
          const neg = r.deltaDb < 0;
          return (
            <div
              key={`${r.name}-${String(i)}`}
              style={{
                display: "grid",
                gridTemplateColumns: "108px 1fr 58px",
                alignItems: "center",
                gap: 8,
                padding: "3px 0",
              }}
            >
              <div
                style={{
                  display: "flex",
                  alignItems: "baseline",
                  gap: 6,
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
          gap: 10,
          marginTop: 10,
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
  );
}

export default SceneConsistency;
