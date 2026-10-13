// src/views/doctor/DiagnosisPanel.tsx — one diagnosis inside an expanded SoundRow: the
// severity-ruled title line (label · kicker · detail · level indicator), the explainer,
// the full BandMeter over the diagnosis's bands, and its prescription card(s).

import { useTheme } from "../../theme/ThemeContext";
import { BandMeter } from "./BandMeter";
import { LevelIndicator } from "./LevelIndicator";
import { PrescriptionCard } from "./PrescriptionCard";
import { diagKicker, isPossible, possibleLabel, sevTone } from "./severity";
import type {
  DoctorDiag,
  DoctorInputArg,
  DoctorSoundResult,
} from "../../lib/types";

export interface DiagnosisPanelProps {
  diag: DoctorDiag;
  sound: DoctorSoundResult;
  /** `${rowId}|${diag.key}` — each prescription card appends its index. */
  cardIdBase: string;
  listIndex: number;
  presetName: string;
  /** The exact input `doctor_check` diagnosed this sound with (see SoundRow). */
  input?: DoctorInputArg;
}

export function DiagnosisPanel({
  diag,
  sound,
  cardIdBase,
  listIndex,
  presetName,
  input,
}: DiagnosisPanelProps) {
  const { t } = useTheme();
  const dTone = sevTone(t, diag.sev);
  const possible = isPossible(diag);
  return (
    <div
      style={{
        borderLeft: `2px solid ${dTone.fg}`,
        paddingLeft: t.space6,
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "baseline",
          gap: t.space4,
          flexWrap: "wrap",
        }}
      >
        <span
          style={{
            fontFamily: t.sans,
            fontSize: t.fsBody,
            fontWeight: 600,
            color: possible ? t.mutedInk : dTone.fg,
          }}
        >
          {possibleLabel(diag)}
        </span>
        <span
          style={{
            fontFamily: t.mono,
            fontSize: t.fsTag,
            letterSpacing: "0.08em",
            textTransform: "uppercase",
            color: possible ? t.mutedInk : dTone.fg,
          }}
        >
          {diagKicker(diag)}
        </span>
        <span
          style={{
            fontFamily: t.mono,
            fontSize: t.fsData2,
            color: t.mutedInk,
          }}
        >
          {diag.detail}
        </span>
        <LevelIndicator
          level={diag.fromLevel}
          sev={diag.sev}
          finding={possibleLabel(diag)}
          size="rich"
        />
      </div>
      <div
        style={{
          fontFamily: t.sans,
          fontSize: t.fsControl,
          lineHeight: 1.5,
          color: t.ink2,
          marginTop: t.space2,
        }}
      >
        {diag.explain}
      </div>
      {diag.bands.length > 0 && (
        <BandMeter
          balanceDb={sound.balanceDb}
          bandLabels={sound.bandLabels}
          bands={diag.bands}
          sev={diag.sev}
        />
      )}
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          gap: t.space5,
          marginTop: t.space5,
        }}
      >
        {diag.rx.map((rx, i) => (
          <PrescriptionCard
            key={`${rx.kind}-${String(i)}`}
            cardId={`${cardIdBase}|${String(i)}`}
            rx={rx}
            listIndex={listIndex}
            presetName={presetName}
            soundScene={sound.scene}
            soundFootswitch={sound.footswitch}
            soundSceneOverrides={sound.sceneOverrides}
            nodes={input?.nodes}
            footswitches={input?.footswitches}
            stimulus={input}
            diag={{ key: diag.key, level: diag.fromLevel, label: diag.label }}
          />
        ))}
      </div>
    </div>
  );
}
