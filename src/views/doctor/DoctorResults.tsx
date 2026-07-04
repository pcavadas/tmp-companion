// src/views/doctor/DoctorResults.tsx — the Doctor RESULTS page: a summary header
// over a worst-first, flat list of per-preset cards. This file owns only the page
// shell (step rail, summary counts, scrollable card list, footer) and the shared
// open-chip state; each card and its parts live in their own files
// (PresetResultCard → SoundRow → DiagnosisChip → BandMeter / PrescriptionCard,
// plus SceneConsistency). Nothing is written to the unit until a prescription is
// applied + saved (each backup-gated, revertible).

import { useCallback, useState } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { Button } from "../../ui/primitives";
import { slotLabel } from "../../lib/format";
import { StepRail } from "../overlays/WizardShell";
import { DOCTOR_STEPS } from "./useDoctorFlow";
import { PresetResultCard } from "./PresetResultCard";
import { presetLookCount, presetWorstSev, sevRank } from "./severity";
import type { DoctorCheckResult } from "../../lib/types";

export interface DoctorResultsProps {
  result: DoctorCheckResult;
  /** 0-based list index → preset name (for the per-preset card headers). */
  presetNames: Map<number, string>;
  onCheckMore: () => void;
}

export function DoctorResults({
  result,
  presetNames,
  onCheckMore,
}: DoctorResultsProps) {
  const { t } = useTheme();
  const [openChips, setOpenChips] = useState<Set<string>>(new Set());

  const toggleChip = useCallback((id: string) => {
    setOpenChips((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const totalSounds = result.presets.reduce((a, p) => a + p.sounds.length, 0);
  const totalPresets = result.presets.length;
  const flagged = result.presets.filter((p) => presetLookCount(p) > 0).length;
  const anyError = result.presets.some((p) =>
    p.sounds.some((s) => s.error != null),
  );
  const allClear = flagged === 0 && !anyError;
  const soundsFlagged = result.presets.reduce(
    (a, p) => a + p.sounds.filter((s) => s.diags.length > 0).length,
    0,
  );
  const needAttention = result.presets.reduce(
    (a, p) =>
      a + p.sounds.filter((s) => s.diags.some((d) => d.sev === "high")).length,
    0,
  );

  // Worst-first: higher severity first, ties broken by slot ascending.
  const sorted = [...result.presets].sort((a, b) => {
    const r = sevRank(presetWorstSev(b)) - sevRank(presetWorstSev(a));
    return r !== 0 ? r : a.listIndex - b.listIndex;
  });

  const title = allClear
    ? `All ${String(totalSounds)} sounds sound good`
    : `${String(flagged)} of ${String(totalPresets)} presets need a look`;

  let subtitle: string;
  if (allClear) {
    subtitle = "Nothing to fix — Doctor didn't find any tone problems.";
  } else {
    subtitle = `Worst first · ${String(soundsFlagged)} sound${soundsFlagged === 1 ? "" : "s"} flagged`;
    if (needAttention > 0) {
      subtitle += ` · ${String(needAttention)} need${needAttention === 1 ? "s" : ""} attention`;
    }
    subtitle += ". Tap a tag to see what it means and fix it.";
  }

  return (
    <div
      style={{
        position: "absolute",
        inset: 0,
        zIndex: 40,
        display: "flex",
        flexDirection: "column",
        background: t.bg,
        color: t.ink,
        fontFamily: t.sans,
      }}
    >
      <div
        style={{
          flexShrink: 0,
          padding: "15px 22px",
          borderBottom: `0.5px solid ${t.hairline}`,
          background: t.bgAlt,
        }}
      >
        <StepRail current={2} steps={DOCTOR_STEPS} />
      </div>

      <div
        style={{
          flexShrink: 0,
          display: "flex",
          gap: 14,
          alignItems: "flex-start",
          padding: "18px 22px 14px",
        }}
      >
        <div
          style={{
            width: 40,
            height: 40,
            borderRadius: 10,
            background: allClear ? t.goodSoft : t.accentSoft,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            flexShrink: 0,
          }}
        >
          <Icon
            name={allClear ? "check" : "wave"}
            size={20}
            stroke={allClear ? t.good : t.warn}
          />
        </div>
        <div style={{ minWidth: 0 }}>
          <div
            style={{ fontFamily: t.serif, fontSize: t.fsCard, color: t.ink }}
          >
            {title}
          </div>
          <div
            style={{
              fontFamily: t.sans,
              fontSize: t.fsLabel,
              color: t.mutedInk,
              marginTop: 3,
              lineHeight: 1.5,
            }}
          >
            {subtitle}
          </div>
        </div>
      </div>

      <div
        style={{
          flex: 1,
          minHeight: 0,
          overflowY: "auto",
          padding: "0 22px 16px",
          display: "flex",
          flexDirection: "column",
          gap: 12,
        }}
      >
        {sorted.map((preset) => (
          <PresetResultCard
            key={preset.listIndex}
            preset={preset}
            presetName={
              presetNames.get(preset.listIndex) ??
              `Slot ${slotLabel(preset.listIndex)}`
            }
            openChips={openChips}
            onToggleChip={toggleChip}
          />
        ))}
      </div>

      <div
        style={{
          flexShrink: 0,
          borderTop: `0.5px solid ${t.hairline}`,
          background: t.bgAlt,
          padding: "12px 22px",
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: 12,
        }}
      >
        <span
          style={{ fontFamily: t.sans, fontSize: t.fsLabel, color: t.mutedInk }}
        >
          Applied fixes are saved per prescription — nothing here is written
          until you save it.
        </span>
        <Button variant="primary" icon="refresh" onClick={onCheckMore}>
          Check other sounds
        </Button>
      </div>
    </div>
  );
}

export default DoctorResults;
