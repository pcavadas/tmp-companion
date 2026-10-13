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
import { slotLabel } from "../../lib/format";
import { WizardPage } from "../overlays/WizardPage";
import { DOCTOR_STEPS } from "./useDoctorFlow";
import { PresetResultCard } from "./PresetResultCard";
import {
  presetLookCount,
  presetWorstSev,
  sceneConsistencySev,
  sevRank,
} from "./severity";
import { ApplyLockContext, useApplyLockOwner } from "./applyLock";
import type {
  DoctorCheckResult,
  DoctorInputArg,
  DoctorPresetResult,
  DoctorSoundResult,
  SilenceHint,
} from "../../lib/types";

type Filter = "look" | "all";

/** A preset the "Needs a look" filter should keep visible: it has a diagnosis /
 *  scene finding, or a sound that errored or was never checked (a stopped run). */
function hasIssue(p: DoctorPresetResult): boolean {
  return (
    presetLookCount(p) > 0 || p.sounds.some((s) => s.status !== "measured")
  );
}

export interface DoctorResultsProps {
  result: DoctorCheckResult;
  /** 0-based list index → preset name (for the per-preset card headers). */
  presetNames: Map<number, string>;
  /** Sound key → the exact `doctor_check` input it was diagnosed with: its
   *  chain, its FULL footswitch roster and its stimulus. Every prescription
   *  card's A/B replays it, so the audition forces off the same blocks the
   *  diagnosis did; an FS sound's own switch (the "shared block" caption) is
   *  found in the same roster. */
  inputByKey: Map<string, DoctorInputArg>;
  /** 0-based list index → the preset's backup-scan silence hint (same source
   *  the Level tab uses) — refines a sound's "no signal captured" error into
   *  a specific JSON-visible cause. */
  silenceHintByIndex: Map<number, SilenceHint>;
  onCheckMore: () => void;
}

export function DoctorResults({
  result,
  presetNames,
  inputByKey,
  silenceHintByIndex,
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

  // ONE applied-but-unsaved prescription across the whole page — the device has
  // a single edit buffer, so a second card's apply (even in another preset)
  // would clobber the first card's live edit. Hence the lock, and every card's
  // lifecycle, lives here, not per card or per preset card.
  const lock = useApplyLockOwner();
  const { busy, holder, discardIfMine } = lock;

  // An applied-but-unsaved edit sits in the DEVICE's edit buffer — leaving this
  // page (unmount, or "Check other sounds" → flow.reset) must drop it, or the
  // orphaned edit silently rides the next preset interaction. `discardIfMine`
  // no-ops for a card that is mid-save, and whichever of this cleanup and the
  // card's own runs first wins.
  const holderRef = useRef<string | null>(null);
  useEffect(() => {
    holderRef.current = holder;
  }, [holder]);
  const discardActive = useCallback(() => {
    const cur = holderRef.current;
    if (cur !== null) discardIfMine(cur);
  }, [discardIfMine]);
  useEffect(() => {
    return discardActive; // unmount only (discardActive is stable)
  }, [discardActive]);

  const handleCheckMore = useCallback(() => {
    if (busy) return;
    discardActive();
    onCheckMore();
  }, [busy, discardActive, onCheckMore]);

  const setReference = useCallback(
    (id: string) => {
      if (!busy) setReferenceId(id);
    },
    [busy],
  );
  const clearReference = useCallback(() => {
    if (!busy) setReferenceId(null);
  }, [busy]);
  const changeFilter = useCallback(
    (f: Filter) => {
      if (!busy) setFilter(f);
    },
    [busy],
  );

  // While a device command is in flight for a card, nothing on the page may
  // unmount it: a collapsed row, a swapped Match reference or a filter that
  // hides its preset would orphan the in-flight apply/save.
  const toggleRow = useCallback(
    (id: string) => {
      if (busy) return;
      setExpanded((prev) => {
        const next = new Set(prev);
        if (next.has(id)) next.delete(id);
        else next.add(id);
        return next;
      });
    },
    [busy],
  );

  const totalSounds = result.presets.reduce((a, p) => a + p.sounds.length, 0);
  const totalPresets = result.presets.length;
  const flagged = result.presets.filter((p) => presetLookCount(p) > 0).length;
  const unrunCount = result.presets.reduce(
    (a, p) => a + p.sounds.filter((s) => s.status === "unrun").length,
    0,
  );
  // "All good" claims every sound was heard — an errored or unrun one wasn't.
  const allClear =
    flagged === 0 &&
    result.presets.every((p) => p.sounds.every((s) => s.status === "measured"));
  const soundsFlagged = result.presets.reduce(
    (a, p) => a + p.sounds.filter((s) => s.diags.length > 0).length,
    0,
  );
  // Presets whose ONLY finding is a scene-loudness jump (no sound diags) still
  // need surfacing — the subtitle's "N of M sounds flagged" clause can't see
  // them (sceneConsistency is a sibling of sounds, not a sound diag).
  const sceneFlagged = result.presets.filter(
    (p) => p.sceneConsistency != null,
  ).length;
  const needAttention = result.presets.reduce((a, p) => {
    const soundHigh = p.sounds.filter((s) =>
      s.diags.some((d) => d.sev === "high"),
    ).length;
    const sceneHigh =
      p.sceneConsistency && sceneConsistencySev(p.sceneConsistency) === "high"
        ? 1
        : 0;
    return a + soundHigh + sceneHigh;
  }, 0);

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

  // A stopped run that flagged nothing has not earned "N of M need a look" (0
  // of M, under a warn icon) — nor "all good", since part of it was never heard.
  const stoppedClean = !allClear && flagged === 0 && result.stopped;
  const title = allClear
    ? `All ${String(totalSounds)} sound${totalSounds === 1 ? " sounds" : "s sound"} good`
    : stoppedClean
      ? "Check stopped · nothing flagged so far"
      : `${String(flagged)} of ${String(totalPresets)} presets need a look`;

  let subtitle: string;
  if (allClear) {
    subtitle = "Nothing to fix — Doctor didn't find any tone problems.";
  } else {
    subtitle = "Worst first";
    if (soundsFlagged > 0) {
      // "of M" counts only the sounds the run reached — an unrun one wasn't checked.
      const checked = totalSounds - unrunCount;
      subtitle += ` · ${String(soundsFlagged)} of ${String(checked)} sound${checked === 1 ? "" : "s"} flagged`;
    }
    if (sceneFlagged > 0) {
      subtitle += ` · level jumps in ${String(sceneFlagged)} preset${sceneFlagged === 1 ? "" : "s"}`;
    }
    if (needAttention > 0) {
      subtitle += ` · ${String(needAttention)} need${needAttention === 1 ? "s" : ""} attention`;
    }
    if (unrunCount > 0) {
      subtitle += ` · ${String(unrunCount)} not checked (the run was stopped)`;
    }
    subtitle += ". Open a row to see what it means and fix it.";
  }

  return (
    <ApplyLockContext.Provider value={lock}>
      <WizardPage steps={DOCTOR_STEPS} current={2}>
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
              background: allClear
                ? t.goodSoft
                : stoppedClean
                  ? t.bgAlt
                  : t.accentSoft,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              flexShrink: 0,
            }}
          >
            <Icon
              name={allClear ? "check" : "wave"}
              size={20}
              stroke={allClear ? t.good : stoppedClean ? t.mutedInk : t.warn}
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
                onChange={changeFilter}
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
            <Button
              variant="ghost"
              size="sm"
              disabled={busy}
              onClick={clearReference}
            >
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
              inputByKey={inputByKey}
              silenceHintByIndex={silenceHintByIndex}
              expanded={expanded}
              onToggleRow={toggleRow}
              referenceSound={referenceSound}
              referenceId={referenceId}
              onSetReference={setReference}
              onClearReference={clearReference}
            />
          ))}
          {showFilter && filter === "look" && (
            <Button
              variant="bare"
              onClick={() => {
                changeFilter("all");
              }}
              style={{
                textAlign: "left",
                display: "flex",
                alignItems: "center",
                gap: t.space4,
                padding: `${String(t.space5)}px ${String(t.space7)}px`,
                borderRadius: 10,
                border: `0.5px dashed ${t.hairlineStrong}`,
                background: t.bgAlt,
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
            </Button>
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
            Nothing here is written until you save it. Each fix has its own
            Save.
          </span>
          <Button
            variant="primary"
            icon="refresh"
            disabled={busy}
            onClick={handleCheckMore}
          >
            Check other sounds
          </Button>
        </div>
      </WizardPage>
    </ApplyLockContext.Provider>
  );
}
