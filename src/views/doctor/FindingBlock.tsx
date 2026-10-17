// src/views/doctor/FindingBlock.tsx — one finding inside an opened sound (look B).
//
// Its name (15/600, "Muddy (borderline)" when it only just fired) with, at the right, when
// it's heard ("Heard at every volume") and the three venue icons — Quiet, Rehearsal,
// Stage — lit in the finding's tone where it fires; then its one-sentence explanation; then
// two columns: the band meter, and "What to change" with the fixes as bullets, simplest
// first, each a step the player makes on the unit (the backend's words, in its order).
// The two columns stack when the panel is too narrow for both. The venue icons are a
// picture of the words beside them, so they are hidden from assistive tech.

import type { ReactNode } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { sectionLabel } from "../../theme/tokens";
import { Icon, type IconName } from "../../ui/Icon";
import type {
  DoctorDiag,
  DoctorSoundResult,
  PlaybackLevel,
} from "../../lib/types";
import { BandMeter } from "./BandMeter";
import { heardAt } from "./resultsModel";
import { findingTone, possibleLabel } from "./severity";

const VENUES: { name: string; icon: IconName }[] = [
  { name: "Quiet", icon: "headphones" },
  { name: "Rehearsal", icon: "speaker" },
  { name: "Stage", icon: "pulse" },
];

/** Which venues a finding fires at: its quietest level and every louder one. */
function firesAt(level: PlaybackLevel): [boolean, boolean, boolean] {
  if (level === "quiet") return [true, true, true];
  if (level === "rehearsal") return [false, true, true];
  return [false, false, true];
}

export interface FindingBlockProps {
  diag: DoctorDiag;
  sound: DoctorSoundResult;
  /** Anything to add under the block (the "too quiet" line under the last one). */
  footnote?: ReactNode;
}

export function FindingBlock({ diag, sound, footnote }: FindingBlockProps) {
  const { t } = useTheme();
  const tone = findingTone(diag);
  const lit = tone === "danger" ? t.danger : t.cautionLine;
  const fires = firesAt(diag.fromLevel);
  const steps = diag.rx.map((rx) => rx.title);
  const caption = {
    fontSize: t.fontCaption,
    lineHeight: `${String(t.lineCaption)}px`,
    whiteSpace: "nowrap",
  } as const;

  return (
    <div
      data-finding={diag.key}
      style={{
        display: "flex",
        flexDirection: "column",
        gap: t.sp8 + t.sp2,
        padding: `${String(t.sp12 + t.sp2)}px 0`,
        borderTop: `1px solid ${t.separator}`,
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: t.sp12,
          flexWrap: "wrap",
        }}
      >
        <h3
          style={{
            margin: 0,
            fontSize: t.fontLarge,
            lineHeight: `${String(t.lineLarge)}px`,
            fontWeight: t.weightSemibold,
          }}
        >
          {possibleLabel(diag)}
        </h3>
        <span
          style={{ display: "flex", alignItems: "center", gap: t.sp8 + t.sp2 }}
        >
          <span style={{ ...caption, color: t.textSecondary }}>
            {heardAt(diag.fromLevel)}
          </span>
          <span aria-hidden style={{ display: "inline-flex", gap: t.sp12 }}>
            {VENUES.map((v, i) => (
              <span
                key={v.name}
                style={{
                  ...caption,
                  display: "flex",
                  alignItems: "center",
                  gap: t.sp4,
                  color: fires[i] ? t.text : t.textSecondary,
                }}
              >
                <Icon
                  name={v.icon}
                  size={14}
                  stroke={fires[i] ? lit : t.textDisabled}
                  strokeWidth={1.8}
                />
                {v.name}
              </span>
            ))}
          </span>
        </span>
      </div>
      <p
        style={{
          margin: 0,
          fontSize: t.fontBody,
          lineHeight: `${String(t.lineBody)}px`,
        }}
      >
        {diag.explain}
      </p>
      <div
        style={{
          display: "flex",
          flexWrap: "wrap",
          alignItems: "flex-start",
          columnGap: t.sp24,
          rowGap: t.sp12,
        }}
      >
        {diag.bands.length > 0 && (
          <BandMeter
            balanceDb={sound.balanceDb}
            bandLabels={sound.bandLabels}
            bands={diag.bands}
            direction={diag.direction}
            tone={tone}
          />
        )}
        {steps.length > 0 && (
          <div
            style={{
              flex: "1 1 240px",
              minWidth: 0,
              display: "flex",
              flexDirection: "column",
              gap: t.sp4,
            }}
          >
            <span style={sectionLabel(t)}>What to change</span>
            <ul
              style={{
                margin: 0,
                paddingLeft: t.sp16 + t.sp2,
                fontSize: t.fontBody,
                lineHeight: `${String(t.lineBody)}px`,
              }}
            >
              {steps.map((s, i) => (
                <li
                  key={`${String(i)}:${s}`}
                  style={{ padding: `${String(t.sp2)}px 0` }}
                >
                  {s}
                </li>
              ))}
            </ul>
          </div>
        )}
      </div>
      {footnote}
    </div>
  );
}
