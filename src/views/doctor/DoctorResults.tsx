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
import { Tag } from "../../ui/Tag";
import { Button, SegmentedControl } from "../../ui/primitives";
import { doctorDiscard } from "../../lib/invoke";
import { slotLabel } from "../../lib/format";
import { StepRail } from "../overlays/WizardShell";
import { DOCTOR_STEPS } from "./useDoctorFlow";
import { PresetResultCard } from "./PresetResultCard";
import type { DoctorStimulus } from "./PrescriptionCard";
import { presetLookCount, presetWorstSev, sevRank } from "./severity";
import {
  ApplyLockContext,
  type ActiveApplyCard,
  type ApplyLock,
} from "./applyLock";
import type {
  ActiveGraph,
  DoctorCheckResult,
  DoctorPresetResult,
  DoctorSoundResult,
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
  /** 0-based list index → the preset's signal chain, from the SAME startup
   *  backup scan as `footswitchInfo` — threaded into every prescription card
   *  so its A/B captures under the diagnosed sound's own context. */
  graphByIndex: Map<number, ActiveGraph>;
  /** Sound key → the stimulus identity it was diagnosed with (the setup-stage
   *  instrument pick) — the prescription cards' A/B replays it. */
  stimulusByKey: Map<string, DoctorStimulus>;
  onCheckMore: () => void;
}

export function DoctorResults({
  result,
  presetNames,
  footswitchInfo,
  graphByIndex,
  stimulusByKey,
  onCheckMore,
}: DoctorResultsProps) {
  const { t } = useTheme();
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [filter, setFilter] = useState<Filter>("look");

  // The "Match reference" picker: ONE sound, page-wide (any preset in this
  // run, not just its own preset) — every other sound offers to move its
  // spectrum toward it. Keyed by the same `${listIndex}|${sound.key}`
  // composite id the row-expansion state already uses.
  const soundById = useMemo(() => {
    const m = new Map<string, DoctorSoundResult>();
    for (const p of result.presets) {
      for (const s of p.sounds) {
        m.set(`${String(p.listIndex)}|${s.key}`, s);
      }
    }
    return m;
  }, [result]);
  const [referenceId, setReferenceId] = useState<string | null>(null);
  const referenceSound = referenceId
    ? (soundById.get(referenceId) ?? null)
    : null;
  const clearReference = useCallback(() => {
    setReferenceId(null);
  }, []);

  // ONE applied-but-unsaved prescription across the whole page — the device has
  // a single edit buffer, so a second card's apply (even in another preset)
  // would clobber the first card's live edit. Hence the lock lives here, not per
  // preset card.
  const [activeCard, setActiveCard] = useState<ActiveApplyCard | null>(null);

  // An applied-but-unsaved edit sits in the DEVICE's edit buffer — leaving this
  // page (unmount, or "Check other sounds" → flow.reset) must drop it, or the
  // orphaned edit silently rides the next preset interaction. Fire-and-forget
  // (the cancel-lane pattern); the ref is cleared first so the page's own
  // unmount cleanup can't double-fire after the reset path already discarded.
  const activeCardRef = useRef<ActiveApplyCard | null>(null);
  useEffect(() => {
    activeCardRef.current = activeCard;
  }, [activeCard]);

  // The single arbiter for "discard the device edit + drop the lock, but only
  // if `id` is really still the holder" — reads/writes `activeCardRef`
  // directly (not React state) so it's safe to call from BOTH this page's own
  // unmount cleanup AND a PrescriptionCard's unmount cleanup without knowing
  // which one runs first (React doesn't guarantee child-before-parent
  // ordering when a whole subtree unmounts at once): the first call to fire
  // wins, the second sees the ref already cleared and no-ops.
  const discardIfMine = useCallback((id: string, listIndex: number) => {
    if (activeCardRef.current?.id !== id) return;
    activeCardRef.current = null;
    void doctorDiscard(listIndex).catch(() => undefined);
    setActiveCard((cur) => (cur?.id === id ? null : cur));
  }, []);

  const lock = useMemo<ApplyLock>(
    () => ({
      activeCard,
      acquire: (id, listIndex) => {
        setActiveCard({ id, listIndex });
      },
      release: (id) => {
        setActiveCard((cur) => (cur?.id === id ? null : cur));
      },
      discardIfMine,
    }),
    [activeCard, discardIfMine],
  );

  const discardActive = useCallback(() => {
    const cur = activeCardRef.current;
    if (cur) discardIfMine(cur.id, cur.listIndex);
  }, [discardIfMine]);
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
            padding: `${String(t.space8)}px ${String(t.space10)}px`,
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
            gap: t.space7,
            alignItems: "flex-start",
            padding: `${String(t.space8)}px ${String(t.space10)}px ${String(t.space7)}px`,
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
                marginTop: t.space2,
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

        {referenceSound && (
          <div
            style={{
              flexShrink: 0,
              display: "flex",
              alignItems: "center",
              gap: t.space4,
              padding: `0 ${String(t.space10)}px ${String(t.space6)}px`,
            }}
          >
            <Tag tone="accent">{`Reference: ${referenceSound.label}`}</Tag>
            <Button variant="ghost" small onClick={clearReference}>
              Clear
            </Button>
          </div>
        )}

        <div
          style={{
            flex: 1,
            minHeight: 0,
            overflowY: "auto",
            padding: `0 ${String(t.space10)}px ${String(t.space8)}px`,
            display: "flex",
            flexDirection: "column",
            gap: t.space6,
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
              graphByIndex={graphByIndex}
              stimulusByKey={stimulusByKey}
              expanded={expanded}
              onToggleRow={toggleRow}
              referenceSound={referenceSound}
              referenceId={referenceId}
              onSetReference={setReferenceId}
              onClearReference={clearReference}
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
                gap: t.space4,
                padding: `${String(t.space5)}px ${String(t.space7)}px`,
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
            padding: `${String(t.space6)}px ${String(t.space10)}px`,
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            gap: t.space6,
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
