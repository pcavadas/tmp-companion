// src/views/doctor/SoundRow.tsx — one checked sound inside a preset card: its
// label + tag, then either "Sounds good", an error message, or a column of
// diagnosis chips (open state threaded down from the results page).

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { DiagnosisChip } from "./DiagnosisChip";
import type { DoctorSoundResult } from "../../lib/types";

export interface SoundRowProps {
  sound: DoctorSoundResult;
  listIndex: number;
  presetName: string;
  /** The nodes this footswitch sound's own switch toggles; undefined for
   *  Base/scene sounds (drives the "shared block" prescription caption). */
  ownNodeIds?: string[];
  /** First row in the card — skips the top hairline. */
  first: boolean;
  openChips: Set<string>;
  onToggleChip: (id: string) => void;
}

export function SoundRow({
  sound,
  listIndex,
  presetName,
  ownNodeIds,
  first,
  openChips,
  onToggleChip,
}: SoundRowProps) {
  const { t } = useTheme();
  const hasDiags = sound.diags.length > 0;

  return (
    <div
      style={{
        padding: "8px 0",
        borderTop: first ? undefined : `0.5px solid ${t.hairline}`,
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "baseline",
          justifyContent: "space-between",
          gap: 12,
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "baseline",
            gap: 8,
            minWidth: 0,
          }}
        >
          <span
            style={{ fontFamily: t.serif, fontSize: t.fsName, color: t.ink }}
          >
            {sound.label}
          </span>
          {sound.tag != null && sound.tag !== "" && (
            <span
              style={{
                fontFamily: t.mono,
                fontSize: t.fsTag,
                letterSpacing: t.lsTag,
                color: t.accentDeep,
                flexShrink: 0,
              }}
            >
              {sound.tag}
            </span>
          )}
        </div>
        {sound.error == null && !hasDiags && (
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
            Sounds good
          </span>
        )}
      </div>
      {sound.error != null && (
        <div
          style={{
            fontFamily: t.mono,
            fontSize: t.fsData,
            color: t.warn,
            marginTop: 6,
          }}
        >
          {sound.error}
        </div>
      )}
      {hasDiags && (
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            gap: 8,
            marginTop: 8,
          }}
        >
          {sound.diags.map((diag) => {
            const id = `${String(listIndex)}|${sound.key}|${diag.key}`;
            return (
              <DiagnosisChip
                key={diag.key}
                diag={diag}
                balanceDb={sound.balanceDb}
                listIndex={listIndex}
                presetName={presetName}
                ownNodeIds={ownNodeIds}
                open={openChips.has(id)}
                onToggle={() => {
                  onToggleChip(id);
                }}
              />
            );
          })}
        </div>
      )}
    </div>
  );
}

export default SoundRow;
