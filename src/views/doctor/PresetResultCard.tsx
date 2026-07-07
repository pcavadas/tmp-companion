// src/views/doctor/PresetResultCard.tsx — one preset's result card (bespoke,
// flagged design-sync candidate): a severity-tinted header with a status badge over
// its checked sounds, plus the scene-consistency section when present. `flexShrink:0`
// is REQUIRED so the results flex column never compresses a card.

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { slotLabel } from "../../lib/format";
import { SoundRow } from "./SoundRow";
import { SceneConsistency } from "./SceneConsistency";
import { presetLookCount, presetWorstSev, sevRank, sevTone } from "./severity";
import type {
  DoctorPresetResult,
  DoctorSoundResult,
  FootswitchInfo,
} from "../../lib/types";

export interface PresetResultCardProps {
  preset: DoctorPresetResult;
  presetName: string;
  footswitchInfo: Map<number, FootswitchInfo[]>;
  openChips: Set<string>;
  onToggleChip: (id: string) => void;
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
  openChips,
  onToggleChip,
}: PresetResultCardProps) {
  const { t } = useTheme();
  const worst = presetWorstSev(preset);
  const count = presetLookCount(preset);
  const tone = sevTone(t, worst);
  const headerBg = sevRank(worst) > 0 ? tone.soft : t.bgAlt;

  // The apply lock (one applied-but-unsaved prescription app-wide — the device
  // has ONE edit buffer) is provided by DoctorResults, not per preset card.
  return (
    <div
      style={{
        flexShrink: 0,
        borderRadius: 14,
        overflow: "hidden",
        border: `0.5px solid ${t.hairline}`,
        background: t.bg,
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 10,
          padding: "12px 14px",
          background: headerBg,
        }}
      >
        <span
          style={{
            fontFamily: t.mono,
            fontSize: t.fsData,
            color: t.mutedInk,
            flexShrink: 0,
          }}
        >
          {slotLabel(preset.listIndex)}
        </span>
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
              gap: 5,
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
              gap: 5,
              fontFamily: t.sans,
              fontSize: t.fsLabel,
              color: tone.fg,
              flexShrink: 0,
            }}
          >
            <Icon name="warn-tri" size={12} stroke={tone.fg} />
            {`${String(count)} thing${count === 1 ? "" : "s"} to look at`}
          </span>
        )}
      </div>
      <div style={{ padding: "4px 14px 12px" }}>
        {preset.sounds.map((sound, i) => (
          <SoundRow
            key={sound.key}
            sound={sound}
            listIndex={preset.listIndex}
            presetName={presetName}
            ownNodeIds={ownNodeIdsFor(
              sound,
              footswitchInfo.get(preset.listIndex),
            )}
            first={i === 0}
            openChips={openChips}
            onToggleChip={onToggleChip}
          />
        ))}
        {preset.sceneConsistency && (
          <SceneConsistency
            sc={preset.sceneConsistency}
            listIndex={preset.listIndex}
            presetName={presetName}
          />
        )}
      </div>
    </div>
  );
}

export default PresetResultCard;
