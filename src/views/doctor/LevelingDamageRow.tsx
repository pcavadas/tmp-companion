// src/views/doctor/LevelingDamageRow.tsx — leveling-damage advisories as a synthetic
// expandable row inside a preset group (mirrors SceneConsistency.tsx's collapsed/
// expanded shape). BACKUP-SCAN ONLY: zero device captures, so this can render for a
// preset even when every sound's check succeeded cleanly. Copy stays factual — a
// footswitch CAN legitimately sweep any parameter via Pro Control, so this names
// what was observed, never "the old leveler damaged this".

import { useTheme } from "../../theme/ThemeContext";
import { DiagnosisChip } from "./DiagnosisChip";
import { PresetFindingRow } from "./PresetFindingRow";
import { levelingDamageSev } from "./severity";
import type { DoctorLevelingDamageHint } from "../../lib/types";

const KIND_COPY: Record<DoctorLevelingDamageHint["kind"], string> = {
  deletedEffect: "drops to ~0 when engaged — the effect goes silent",
  sweptOther:
    "isn't a level control — engaging it changes tone, not just volume",
};

export interface LevelingDamageRowProps {
  hints: DoctorLevelingDamageHint[];
  open: boolean;
  onToggle: () => void;
}

export function LevelingDamageRow({
  hints,
  open,
  onToggle,
}: LevelingDamageRowProps) {
  const { t } = useTheme();
  if (hints.length === 0) return null;
  const sev = levelingDamageSev(hints);

  return (
    <PresetFindingRow
      sev={sev}
      title="Leveling damage"
      chip={
        <DiagnosisChip
          label={`${String(hints.length)} assignment${hints.length === 1 ? "" : "s"} worth checking`}
          sev={sev}
        />
      }
      open={open}
      onToggle={onToggle}
    >
      <div
        style={{
          fontFamily: t.sans,
          fontSize: 12.5,
          color: t.ink2,
          lineHeight: 1.55,
        }}
      >
        These footswitch assignments don&rsquo;t look like level controls — no
        device capture needed, this is read straight from the preset. Worth a
        listen if they weren&rsquo;t set on purpose.
      </div>
      <div
        style={{
          marginTop: t.space5,
          display: "flex",
          flexDirection: "column",
          gap: t.space4,
        }}
      >
        {hints.map((h, i) => (
          <div
            key={`${String(h.switch)}-${h.nodeId}-${h.parameterId}-${String(i)}`}
            style={{
              display: "flex",
              flexDirection: "column",
              gap: t.space2,
              padding: `${String(t.space3)}px ${String(t.space4)}px`,
              borderRadius: 8,
              background: t.bgAlt,
              border: `0.5px solid ${t.hairline}`,
            }}
          >
            <div
              style={{
                display: "flex",
                alignItems: "baseline",
                gap: t.space3,
                flexWrap: "wrap",
              }}
            >
              <span
                style={{
                  fontFamily: t.sans,
                  fontSize: 12.5,
                  fontWeight: 600,
                  color: t.ink,
                }}
              >
                {h.label || `Switch ${String(h.switch + 1)}`}
              </span>
              <span
                style={{
                  fontFamily: t.mono,
                  fontSize: 10,
                  color: t.mutedInk,
                }}
              >
                {h.detail}
              </span>
            </div>
            <div style={{ fontFamily: t.sans, fontSize: 12, color: t.ink2 }}>
              {KIND_COPY[h.kind]}
            </div>
          </div>
        ))}
      </div>
    </PresetFindingRow>
  );
}

export default LevelingDamageRow;
