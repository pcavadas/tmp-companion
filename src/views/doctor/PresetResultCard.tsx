// src/views/doctor/PresetResultCard.tsx — one preset's result group: a
// severity-tinted header with a status badge, then its sound rows (problems
// worst-first, then errored rows), the synthetic scene-consistency row when present,
// and a collapsed healthy summary that reveals its clear rows on click.
// `flexShrink:0` is REQUIRED so the results flex column never compresses a card.

import { useState } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { SlotLabel } from "../../ui/SlotLabel";
import { SevDot, SoundRow } from "./SoundRow";
import { SceneConsistency } from "./SceneConsistency";
import {
  presetLookCount,
  presetWorstSev,
  sevRank,
  sevTone,
  soundSev,
} from "./severity";
import type {
  DoctorPresetResult,
  DoctorSoundResult,
  FootswitchInfo,
} from "../../lib/types";

export interface PresetResultCardProps {
  preset: DoctorPresetResult;
  presetName: string;
  footswitchInfo: Map<number, FootswitchInfo[]>;
  /** Open row ids, keyed `${listIndex}|${sound.key}` (and `|consistency`). */
  expanded: Set<string>;
  onToggleRow: (id: string) => void;
}

/** The node ids a footswitch SOUND owns — the blocks its own switch toggles.
 *  Base/scene sounds (`footswitch == null`) own nothing (undefined). The `f${slot}:${i}`
 *  key's `i` indexes the preset's `FootswitchInfo[]`. */
function ownNodeIdsFor(
  sound: DoctorSoundResult,
  fsList: FootswitchInfo[] | undefined,
): string[] | undefined {
  if (sound.footswitch == null) return undefined;
  const i = Number(sound.key.split(":")[1]);
  return fsList?.[i]?.functions.map((f) => f.node_id);
}

export function PresetResultCard({
  preset,
  presetName,
  footswitchInfo,
  expanded,
  onToggleRow,
}: PresetResultCardProps) {
  const { t } = useTheme();
  const worst = presetWorstSev(preset);
  const count = presetLookCount(preset);
  const tone = sevTone(t, worst);
  const tinted = sevRank(worst) > 0;
  const [showHealthy, setShowHealthy] = useState(count === 0);

  // Problems worst-first, then errored rows (visible, non-expandable), then the
  // healthy rows (collapsed by default unless the whole group is clear).
  const problems = preset.sounds
    .filter((s) => s.diags.length > 0)
    .sort((a, b) => sevRank(soundSev(b)) - sevRank(soundSev(a)));
  const errored = preset.sounds.filter(
    (s) => s.diags.length === 0 && s.error != null,
  );
  const healthy = preset.sounds.filter(
    (s) => s.diags.length === 0 && s.error == null,
  );
  const visibleProblemRows = [...problems, ...errored];

  const row = (sound: DoctorSoundResult) => {
    const id = `${String(preset.listIndex)}|${sound.key}`;
    return (
      <SoundRow
        key={sound.key}
        sound={sound}
        listIndex={preset.listIndex}
        presetName={presetName}
        ownNodeIds={ownNodeIdsFor(sound, footswitchInfo.get(preset.listIndex))}
        open={expanded.has(id)}
        onToggle={() => {
          onToggleRow(id);
        }}
      />
    );
  };

  const consistencyId = `${String(preset.listIndex)}|consistency`;

  return (
    <div
      style={{
        flexShrink: 0,
        borderRadius: 14,
        overflow: "hidden",
        border: `0.5px solid ${tinted ? tone.border : t.hairlineStrong}`,
        background: t.bg,
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: t.space5,
          padding: `${String(t.space6)}px ${String(t.space7)}px`,
          background: tinted ? tone.soft : t.bgAlt,
        }}
      >
        <SlotLabel index={preset.listIndex} style={{ flexShrink: 0 }} />
        <span
          style={{
            flex: 1,
            minWidth: 0,
            fontFamily: t.serif,
            fontSize: 17,
            color: t.ink,
            whiteSpace: "nowrap",
            overflow: "hidden",
            textOverflow: "ellipsis",
          }}
        >
          {presetName}
        </span>
        {count === 0 ? (
          <span
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: t.space3,
              fontFamily: t.sans,
              fontSize: t.fsLabel,
              color: t.good,
              flexShrink: 0,
            }}
          >
            <Icon name="check" size={12} stroke={t.good} />
            All clear
          </span>
        ) : (
          <span
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: t.space3,
              fontFamily: t.sans,
              fontSize: t.fsLabel,
              color: tone.fg,
              flexShrink: 0,
            }}
          >
            <Icon name="warn-tri" size={12} stroke={tone.fg} />
            {`${String(count)} to look at`}
          </span>
        )}
      </div>
      <div style={{ padding: `0 ${String(t.space4)}px ${String(t.space2)}px` }}>
        {visibleProblemRows.map(row)}
        {preset.sceneConsistency && (
          <SceneConsistency
            sc={preset.sceneConsistency}
            listIndex={preset.listIndex}
            presetName={presetName}
            open={expanded.has(consistencyId)}
            onToggle={() => {
              onToggleRow(consistencyId);
            }}
          />
        )}
        {healthy.length > 0 &&
          (showHealthy ? (
            healthy.map(row)
          ) : (
            <div
              onClick={() => {
                setShowHealthy(true);
              }}
              style={{
                display: "flex",
                alignItems: "center",
                gap: t.space4,
                minHeight: 34,
                padding: `0 ${String(t.space4)}px 0 ${String(t.space3)}px`,
                borderTop: `0.5px solid ${t.hairline}`,
                cursor: "pointer",
              }}
            >
              <SevDot sev="ok" />
              <span
                style={{ fontFamily: t.sans, fontSize: 12, color: t.mutedInk }}
              >
                {`${String(healthy.length)} sound${healthy.length === 1 ? "" : "s"} check${healthy.length === 1 ? "s" : ""} out`}
              </span>
              <span style={{ display: "inline-flex", opacity: 0.6 }}>
                <Icon name="chev-right" size={12} stroke={t.mutedInk} />
              </span>
            </div>
          ))}
      </div>
    </div>
  );
}

export default PresetResultCard;
