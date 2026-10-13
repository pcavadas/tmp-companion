// src/views/doctor/PresetResultCard.tsx — one preset's result group: a
// severity-tinted header with a status badge, then its sound rows (problems
// worst-first, then errored rows), the synthetic scene-consistency row when present,
// and a collapsed healthy summary that reveals its clear rows on click.
// `flexShrink:0` is REQUIRED so the results flex column never compresses a card.

import { useState } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { Button } from "../../ui/primitives";
import { SlotLabel } from "../../ui/SlotLabel";
import { SevDot, SoundRow } from "./SoundRow";
import { SceneConsistency } from "./SceneConsistency";
import { LevelingDamageRow } from "./LevelingDamageRow";
import {
  presetLookCount,
  presetWorstSev,
  sevRank,
  sevTone,
  soundSev,
} from "./severity";
import type {
  DoctorInputArg,
  DoctorPresetResult,
  DoctorSoundResult,
  FootswitchInfo,
  SilenceHint,
} from "../../lib/types";

export interface PresetResultCardProps {
  preset: DoctorPresetResult;
  presetName: string;
  /** Sound key → the exact `doctor_check` input it was diagnosed with (chain,
   *  full footswitch roster, stimulus) — every prescription card's A/B replays
   *  it, so diagnosis and audition share ONE input. */
  inputByKey: Map<string, DoctorInputArg>;
  /** 0-based list index → the preset's backup-scan silence hint (same source
   *  as the Level tab) — refines a sound's "no signal captured" error. */
  silenceHintByIndex: Map<number, SilenceHint>;
  /** Open row ids, keyed `${listIndex}|${sound.key}` (and `|consistency`). */
  expanded: Set<string>;
  onToggleRow: (id: string) => void;
  /** The page-wide picked Match-reference sound (any preset in the run), or
   *  null before one is picked — see `DoctorResults`'s `soundById`. */
  referenceSound: DoctorSoundResult | null;
  referenceId: string | null;
  onSetReference: (id: string) => void;
  onClearReference: () => void;
}

/** The node ids a footswitch SOUND owns — the blocks its own switch toggles.
 *  Base/scene sounds (`footswitch == null`) own nothing (undefined). Matched on
 *  the device switch number, never on a list position: the sound key's `i`
 *  indexes the levelable-only list, not the full roster the input carries. */
function ownNodeIdsFor(
  sound: DoctorSoundResult,
  fsList: FootswitchInfo[] | undefined,
): string[] | undefined {
  if (sound.footswitch == null) return undefined;
  return fsList
    ?.find((f) => f.switch === sound.footswitch)
    ?.functions.map((f) => f.node_id);
}

export function PresetResultCard({
  preset,
  presetName,
  inputByKey,
  silenceHintByIndex,
  expanded,
  onToggleRow,
  referenceSound,
  referenceId,
  onSetReference,
  onClearReference,
}: PresetResultCardProps) {
  const { t } = useTheme();
  const worst = presetWorstSev(preset);
  const count = presetLookCount(preset);
  const tone = sevTone(t, worst);
  const tinted = sevRank(worst) > 0;
  const [showHealthy, setShowHealthy] = useState(count === 0);

  // Problems worst-first, then errored and unrun rows (visible, non-expandable),
  // then the healthy rows (collapsed by default unless the whole group is clear).
  // Only a MEASURED sound with no findings is healthy — an unrun one was never heard.
  const problems = preset.sounds
    .filter((s) => s.diags.length > 0)
    .sort((a, b) => sevRank(soundSev(b)) - sevRank(soundSev(a)));
  const notMeasured = preset.sounds.filter((s) => s.status !== "measured");
  const healthy = preset.sounds.filter(
    (s) => s.status === "measured" && s.diags.length === 0,
  );
  const visibleProblemRows = [...problems, ...notMeasured];
  // "All clear" is a claim about EVERY sound — never made while one wasn't heard.
  const fullyChecked = notMeasured.length === 0;

  const row = (sound: DoctorSoundResult) => {
    const id = `${String(preset.listIndex)}|${sound.key}`;
    const input = inputByKey.get(sound.key);
    return (
      <SoundRow
        key={sound.key}
        sound={sound}
        listIndex={preset.listIndex}
        presetName={presetName}
        ownNodeIds={ownNodeIdsFor(sound, input?.footswitches)}
        input={input}
        silenceHint={silenceHintByIndex.get(preset.listIndex)}
        open={expanded.has(id)}
        onToggle={() => {
          onToggleRow(id);
        }}
        id={id}
        referenceSound={referenceSound}
        referenceId={referenceId}
        onSetReference={onSetReference}
        onClearReference={onClearReference}
      />
    );
  };

  const consistencyId = `${String(preset.listIndex)}|consistency`;
  const damageId = `${String(preset.listIndex)}|leveling-damage`;
  const levelingDamage = preset.levelingDamage;

  return (
    <div
      // e2e hook: scope per-preset selectors (e.g. diagnosis chip counts) to one
      // preset's card, mirroring TargetEditCard's `data-target-card`.
      data-preset-card={presetName}
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
        {count === 0 && !fullyChecked ? (
          <span
            style={{
              fontFamily: t.sans,
              fontSize: t.fsLabel,
              color: t.mutedInk,
              flexShrink: 0,
            }}
          >
            Not fully checked
          </span>
        ) : count === 0 ? (
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
        <LevelingDamageRow
          hints={levelingDamage}
          open={expanded.has(damageId)}
          onToggle={() => {
            onToggleRow(damageId);
          }}
        />
        {healthy.length > 0 &&
          (showHealthy ? (
            healthy.map(row)
          ) : (
            <Button
              variant="bare"
              aria-expanded={false}
              onClick={() => {
                setShowHealthy(true);
              }}
              style={{
                width: "100%",
                textAlign: "left",
                display: "flex",
                alignItems: "center",
                gap: t.space4,
                minHeight: 34,
                padding: `0 ${String(t.space4)}px 0 ${String(t.space3)}px`,
                borderTop: `0.5px solid ${t.hairline}`,
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
            </Button>
          ))}
      </div>
    </div>
  );
}
