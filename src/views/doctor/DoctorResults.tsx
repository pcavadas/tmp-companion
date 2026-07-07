// src/views/doctor/DoctorResults.tsx — the Doctor RESULTS page: a summary header
// over a worst-first, flat list of per-preset cards. This file owns only the page
// shell (step rail, summary counts, scrollable card list, footer) and the shared
// open-chip state; each card and its parts live in their own files
// (PresetResultCard → SoundRow → DiagnosisChip → BandMeter / PrescriptionCard,
// plus SceneConsistency). Nothing is written to the unit until a prescription is
// applied + saved (each backup-gated, revertible).

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { Button, SegmentedControl } from "../../ui/primitives";
import { doctorDiscard } from "../../lib/invoke";
import { slotLabel } from "../../lib/format";
import { StepRail } from "../overlays/WizardShell";
import { DOCTOR_STEPS } from "./useDoctorFlow";
import { PresetResultCard } from "./PresetResultCard";
import { presetLookCount, presetWorstSev, sevRank } from "./severity";
import {
  ApplyLockContext,
  type ActiveApplyCard,
  type ApplyLock,
} from "./applyLock";
import type {
  DoctorCheckResult,
  DoctorPresetResult,
  FootswitchInfo,
} from "../../lib/types";

type Filter = "look" | "all";

/** A preset the "Needs a look" filter should keep visible: it has a diagnosis /
 *  scene finding, or an errored sound the player needs to see. */
function hasIssue(p: DoctorPresetResult): boolean {
  return presetLookCount(p) > 0 || p.sounds.some((s) => s.error != null);
}

export interface DoctorResultsProps {
  result: DoctorCheckResult;
  /** 0-based list index → preset name (for the per-preset card headers). */
  presetNames: Map<number, string>;
  /** 0-based list index → the preset's block-acting footswitches (their toggled
   *  nodes drive the "shared block" caption on FS-sound prescriptions). */
  footswitchInfo: Map<number, FootswitchInfo[]>;
  onCheckMore: () => void;
}

export function DoctorResults({
  result,
  presetNames,
  footswitchInfo,
  onCheckMore,
}: DoctorResultsProps) {
  const { t } = useTheme();
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [filter, setFilter] = useState<Filter>("look");

  // ONE applied-but-unsaved prescription across the whole page — the device has
  // a single edit buffer, so a second card's apply (even in another preset)
  // would clobber the first card's live edit. Hence the lock lives here, not per
  // preset card.
  const [activeCard, setActiveCard] = useState<ActiveApplyCard | null>(null);
  const lock = useMemo<ApplyLock>(
    () => ({
      activeCard,
      acquire: (id, listIndex) => {
        setActiveCard({ id, listIndex });
      },
      release: (id) => {
        setActiveCard((cur) => (cur?.id === id ? null : cur));
      },
    }),
    [activeCard],
  );

  // An applied-but-unsaved edit sits in the DEVICE's edit buffer — leaving this
  // page (unmount, or "Check other sounds" → flow.reset) must drop it, or the
  // orphaned edit silently rides the next preset interaction. Fire-and-forget
  // (the cancel-lane pattern); the ref is cleared first so the unmount cleanup
  // can't double-fire after the reset path already discarded.
  const activeCardRef = useRef<ActiveApplyCard | null>(null);
  useEffect(() => {
    activeCardRef.current = activeCard;
  }, [activeCard]);
  const discardActive = useCallback(() => {
    const cur = activeCardRef.current;
    activeCardRef.current = null;
    if (cur) {
      void doctorDiscard(cur.listIndex).catch(() => undefined);
    }
  }, []);
  useEffect(() => {
    return discardActive; // unmount only (discardActive is stable)
  }, [discardActive]);

  const handleCheckMore = useCallback(() => {
    discardActive();
    onCheckMore();
  }, [discardActive, onCheckMore]);

  const toggleRow = useCallback((id: string) => {
    setExpanded((prev) => {
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

  // "Needs a look" hides fully-clean presets; the segmented control + the
  // "sound good · Show all" strip only appear when there is one to hide.
  const cleanGroups = sorted.filter((p) => !hasIssue(p));
  // All-clear is the happy path: show every card, no filtering, no strip.
  const shown = allClear || filter === "all" ? sorted : sorted.filter(hasIssue);
  const showFilter = !allClear && cleanGroups.length > 0;

  const title = allClear
    ? `All ${String(totalSounds)} sound${totalSounds === 1 ? " sounds" : "s sound"} good`
    : `${String(flagged)} of ${String(totalPresets)} presets need a look`;

  let subtitle: string;
  if (allClear) {
    subtitle = "Nothing to fix — Doctor didn't find any tone problems.";
  } else {
    subtitle = `Worst first · ${String(soundsFlagged)} of ${String(totalSounds)} sound${totalSounds === 1 ? "" : "s"} flagged`;
    if (needAttention > 0) {
      subtitle += ` · ${String(needAttention)} need${needAttention === 1 ? "s" : ""} attention`;
    }
    subtitle += ". Open a row to see what it means and fix it.";
  }

  return (
    <ApplyLockContext.Provider value={lock}>
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
          <div style={{ flex: 1, minWidth: 0 }}>
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
          {showFilter && (
            <div style={{ flexShrink: 0 }}>
              <SegmentedControl<Filter>
                size="sm"
                ariaLabel="Filter results"
                value={filter}
                onChange={setFilter}
                options={[
                  { value: "look", label: "Needs a look" },
                  { value: "all", label: "Everything" },
                ]}
              />
            </div>
          )}
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
          {shown.map((preset) => (
            <PresetResultCard
              key={preset.listIndex}
              preset={preset}
              presetName={
                presetNames.get(preset.listIndex) ??
                `Slot ${slotLabel(preset.listIndex)}`
              }
              footswitchInfo={footswitchInfo}
              expanded={expanded}
              onToggleRow={toggleRow}
            />
          ))}
          {showFilter && filter === "look" && (
            <div
              onClick={() => {
                setFilter("all");
              }}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 9,
                padding: "10px 14px",
                borderRadius: 10,
                border: `0.5px dashed ${t.hairlineStrong}`,
                background: t.bgAlt,
                cursor: "pointer",
              }}
            >
              <Icon name="check" size={14} stroke={t.good} />
              <span
                style={{
                  fontFamily: t.sans,
                  fontSize: 12.5,
                  color: t.ink2,
                  whiteSpace: "nowrap",
                }}
              >
                {`${String(cleanGroups.length)} preset${cleanGroups.length === 1 ? "" : "s"} sound${cleanGroups.length === 1 ? "s" : ""} good`}
              </span>
              <span style={{ flex: 1 }} />
              <span
                style={{
                  fontFamily: t.mono,
                  fontSize: 10,
                  letterSpacing: "0.06em",
                  textTransform: "uppercase",
                  color: t.mutedInk,
                }}
              >
                Show all
              </span>
            </div>
          )}
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
            style={{
              fontFamily: t.sans,
              fontSize: t.fsLabel,
              color: t.mutedInk,
            }}
          >
            Applied fixes are saved per prescription — nothing here is written
            until you save it.
          </span>
          <Button variant="primary" icon="refresh" onClick={handleCheckMore}>
            Check other sounds
          </Button>
        </div>
      </div>
    </ApplyLockContext.Provider>
  );
}

export default DoctorResults;
