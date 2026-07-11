// src/views/overlays/SetupBody.tsx — wizard step 2, "Set up".
//
// Everything chosen in the LIST (the scene tree) WILL be leveled — this step never
// re-gates inclusion. Its single job is to set each sound's INSTRUMENT + TARGET:
//   • A top "Apply to" bar is a brush that writes to every row at once — or, when the
//     user ticks a few rows, to just those. Ticking is a bulk-edit convenience only.
//   • Each row also carries its OWN instrument + target pickers.
// On "Level N sounds" it hands the flow one SetupChoice per option. The footer's
// "I've backed up with Pro Control" checkbox gates the button (an inline backup
// acknowledgment — there is no separate Back-up step). Re-level skips the ack (the
// user already acknowledged when the initial run started).
//
// History (do not reintroduce): an earlier build put inclusion checkboxes here,
// forcing users to pick sounds twice (list + dialog). The list is the single place
// you choose WHAT to level; this step only chooses HOW.

import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";

import { useTheme, useStyles } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { Button, Toggle } from "../../ui/primitives";
import { BackupAckLabel } from "../../ui/BackupAckLabel";
import { SetupGroupHeader } from "../../ui/SetupGroupHeader";
import { PresetOptionRow } from "../../ui/PresetOptionRow";
import { ApplyToBar } from "../../ui/ApplyToBar";
import { usePickedRows } from "../../lib/usePickedRows";
import { WizardFooter, WizTitle } from "./WizardShell";
import { ByEarChip } from "./ByEarChip";
import { Pick, type PickOption } from "./Pick";
import { FsParamPick } from "./FsParamPick";
import {
  defaultParamIndex,
  instCalState,
  targetFromCandidate,
} from "../level/leveling";
import type { SetupOption, SetupChoice } from "../level/leveling";

export type { SetupChoice };

/** The "calibrate" word — an inviting next-step cue, never a button. Dotted terracotta
 *  underline that solidifies on hover. The pointer + title point the way to calibration;
 *  it carries no nav itself (no dead reload). */
// ponytail: hint-only cue. To make it actually jump to Settings → Instruments, thread an
// onCalibrate callback from App down through LevelView/LevelingWizard and call it here.
function CalibrateCue({ children }: { children: ReactNode }) {
  const { t } = useTheme();
  const [hover, setHover] = useState(false);
  return (
    <span
      title="Calibrate instruments in Settings"
      onMouseEnter={() => {
        setHover(true);
      }}
      onMouseLeave={() => {
        setHover(false);
      }}
      style={{
        color: t.accentDeep,
        fontWeight: 500,
        cursor: "pointer",
        textDecoration: "underline",
        textDecorationStyle: "dotted",
        textDecorationColor: hover ? t.accentDeep : t.warnBorder,
        textUnderlineOffset: "2.5px",
      }}
    >
      {children}
    </span>
  );
}

/** Quiet good → better → best caption beneath the apply-to-all instrument picker.
 *  `cal` removes the element entirely (no reserved height) so the list below reclaims
 *  the space. Not a warning — muted body with a single accent cue on "calibrate". */
function InstrumentNudge({ state }: { state: "none" | "uncal" | "cal" }) {
  const { t } = useTheme();
  if (state === "cal") return null;
  return (
    <div
      aria-live="polite"
      style={{
        marginTop: 9,
        fontFamily: t.sans,
        fontSize: 12,
        lineHeight: 1.45,
        color: t.mutedInk,
      }}
    >
      {state === "none" ? (
        <span>
          Set an instrument for better results —{" "}
          <CalibrateCue>calibrate</CalibrateCue> it for the best.
        </span>
      ) : (
        <span>
          <CalibrateCue>Calibrate</CalibrateCue> this instrument for the best
          results.
        </span>
      )}
    </div>
  );
}

/** Onboarding nudge toward Tier-2 calibration (capture-as-stimulus) — a small
 *  dismissable banner shown once per wizard open, only while the chosen instrument
 *  is a real, uncalibrated profile (an unset/"None" instrument or an already-
 *  calibrated one shows nothing). Local `dismissed` state, so re-entering the Set
 *  up step (a fresh SetupBody mount) shows it again — cheap enough not to thread
 *  through the flow. No navigation coupling: plain text points at Settings. */
function CalibrationOnboardingBanner({ show }: { show: boolean }) {
  const { t } = useTheme();
  const [dismissed, setDismissed] = useState(false);
  if (!show || dismissed) return null;
  return (
    <div
      role="status"
      style={{
        flexShrink: 0,
        display: "flex",
        alignItems: "flex-start",
        gap: 9,
        margin: "12px 24px 0",
        padding: "9px 11px",
        borderRadius: t.rCard,
        border: `0.5px solid ${t.hairlineStrong}`,
        background: t.bgAlt,
      }}
    >
      <span style={{ display: "flex", flexShrink: 0, marginTop: 1 }}>
        <Icon name="info" size={14} stroke={t.accentDeep} strokeWidth={1.5} />
      </span>
      <span
        style={{
          flex: 1,
          fontFamily: t.sans,
          fontSize: 12,
          lineHeight: 1.45,
          color: t.ink2,
        }}
      >
        Level with your own guitar — a 2-minute calibration makes leveling match
        your instrument. Settings → Instruments → Calibrate.
      </span>
      <button
        type="button"
        aria-label="Dismiss"
        title="Dismiss"
        onClick={() => {
          setDismissed(true);
        }}
        style={{
          cursor: "pointer",
          display: "flex",
          flexShrink: 0,
          background: "transparent",
          border: 0,
          padding: 0,
        }}
      >
        <Icon name="x" size={12} stroke={t.mutedInk} />
      </button>
    </div>
  );
}

export interface SetupBodyProps {
  /** The exact scenes picked in the list — all of them WILL be leveled. */
  options: SetupOption[];
  /** How many presets the flow is leveling (for the sub-line). */
  presetCount: number;
  /** True ⇒ re-leveling a clamped subset (title prefix + backup ack hidden). */
  isRelevel: boolean;
  instrumentOptions: PickOption[];
  targetOptions: PickOption[];
  /** Store-backed defaults (never hard-coded ids). */
  defaultInst: string;
  defaultTarget: string;
  onCancel: () => void;
  onStart: (choices: SetupChoice[]) => void;
  /** Opt-in: equalize a path-MERGE preset's two parallel-amp lanes before leveling.
   * A no-op on series / single-amp / split-output presets. */
  onRebalanceChange?: (on: boolean) => void;
}

export function SetupBody({
  options,
  presetCount,
  isRelevel,
  instrumentOptions,
  targetOptions,
  defaultInst,
  defaultTarget,
  onCancel,
  onStart,
  onRebalanceChange,
}: SetupBodyProps) {
  const { t } = useTheme();
  const s = useStyles();
  // Inline backup acknowledgment — gates the primary button (mirrors the Copy save
  // bar). Required only on a fresh run; re-level already acknowledged. Default off.
  const requireBackup = !isRelevel;
  const [backedUp, setBackedUp] = useState(false);
  // Advanced, opt-in run option — applies to the whole run; default off. Toggling it
  // both updates the local pill and notifies the flow (read at run time as `rebalance`).
  const [rebalance, setRebalance] = useState(false);
  const toggleRebalance = () => {
    const next = !rebalance;
    setRebalance(next);
    onRebalanceChange?.(next);
  };
  // The flow holds `rebalance` in a ref that survives this body's unmount/remount, but
  // the pill resets to its default each mount — sync the ref to the VISIBLE state on
  // mount so a stale ON from a prior run (re-level / Back→Continue / a new flow) can't
  // silently rebalance against an OFF-looking pill.
  const didSyncRebalance = useRef(false);
  useEffect(() => {
    if (didSyncRebalance.current) return;
    didSyncRebalance.current = true;
    onRebalanceChange?.(rebalance);
  }, [onRebalanceChange, rebalance]);

  const groups = useMemo(() => {
    const by = new Map<
      number,
      { slot: number; name: string; opts: SetupOption[] }
    >();
    options.forEach((o) => {
      let group = by.get(o.slot);
      if (!group) {
        group = { slot: o.slot, name: o.presetName, opts: [] };
        by.set(o.slot, group);
      }
      group.opts.push(o);
    });
    return [...by.values()].sort((a, b) => a.slot - b.slot);
  }, [options]);

  // Per-row instrument + target — the real values that get leveled.
  const [rowInst, setRowInst] = useState<Record<string, string>>(() => {
    const m: Record<string, string> = {};
    options.forEach((o) => (m[o.key] = defaultInst));
    return m;
  });
  const [rowTarget, setRowTarget] = useState<Record<string, string>>(() => {
    const m: Record<string, string> = {};
    options.forEach((o) => (m[o.key] = defaultTarget));
    return m;
  });
  const setOneInst = (k: string, v: string) => {
    setRowInst((p) => ({ ...p, [k]: v }));
  };
  const setOneTarget = (k: string, v: string) => {
    setRowTarget((p) => ({ ...p, [k]: v }));
  };

  // Which block parameter levels each footswitch (index into the row's `levelParams`).
  // Seeded with the tone-safe default; only footswitch rows with candidates appear.
  const [rowParam, setRowParam] = useState<Record<string, number>>(() => {
    const m: Record<string, number> = {};
    options.forEach((o) => {
      if (o.footswitch != null && o.levelParams && o.levelParams.length > 0)
        m[o.key] = defaultParamIndex(o.levelParams);
    });
    return m;
  });
  const setOneParam = (k: string, i: number) => {
    setRowParam((p) => ({ ...p, [k]: i }));
  };

  // Bulk-edit selection (which rows the "Apply to" bar writes to). Empty = all.
  const {
    picked,
    togglePick,
    clearPicked,
    somePicked,
    targetsForBulk,
    scopeLabel,
  } = usePickedRows(options);

  // The "Apply to" bar's current value (also the brush applied on change).
  const [bulkInst, setBulkInst] = useState(defaultInst);
  const [bulkTarget, setBulkTarget] = useState(defaultTarget);
  const applyBulkInst = (v: string) => {
    setBulkInst(v);
    setRowInst((p) => {
      const n = { ...p };
      targetsForBulk().forEach((k) => (n[k] = v));
      return n;
    });
  };
  const applyBulkTarget = (v: string) => {
    setBulkTarget(v);
    setRowTarget((p) => {
      const n = { ...p };
      targetsForBulk().forEach((k) => (n[k] = v));
      return n;
    });
  };

  const total = options.length;

  const start = () => {
    const choices: SetupChoice[] = options.map((o) => {
      // Bake the chosen candidate into the footswitch row's leveling target; Base +
      // scene rows pass through unchanged.
      let option = o;
      if (o.footswitch != null && o.levelParams && o.levelParams.length > 0) {
        const idx = rowParam[o.key];
        if (idx >= 0 && idx < o.levelParams.length)
          option = {
            ...o,
            footswitch: targetFromCandidate(
              o.footswitch.switchIndex,
              o.levelParams[idx],
            ),
          };
      }
      return {
        option,
        instId: rowInst[o.key] ?? defaultInst,
        targetName: rowTarget[o.key] ?? defaultTarget,
      };
    });
    if (choices.length) onStart(choices);
  };

  return (
    <>
      <div
        style={{
          flexShrink: 0,
          padding: "16px 24px 13px",
          borderBottom: `0.5px solid ${t.hairline}`,
        }}
      >
        <WizTitle>
          {isRelevel
            ? "Re-level — set instrument & target"
            : "Set instrument & target"}
        </WizTitle>
        <div
          style={{
            fontFamily: t.mono,
            fontSize: 10.5,
            letterSpacing: "0.04em",
            color: t.mutedInk,
            marginTop: 7,
          }}
        >
          {total} sound{total === 1 ? "" : "s"} · {presetCount} preset
          {presetCount === 1 ? "" : "s"}
        </div>
      </div>

      <CalibrationOnboardingBanner
        show={instCalState(bulkInst, instrumentOptions) === "uncal"}
      />

      {/* apply-to bar — writes to all rows, or to the ticked rows */}
      <ApplyToBar
        label={`Apply to ${scopeLabel}`}
        somePicked={somePicked}
        onClear={clearPicked}
      >
        <div
          style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 16 }}
        >
          <Pick
            grow
            value={bulkInst}
            options={instrumentOptions}
            onChange={applyBulkInst}
          />
          <Pick
            grow
            value={bulkTarget}
            options={targetOptions}
            onChange={applyBulkTarget}
          />
        </div>
        <InstrumentNudge state={instCalState(bulkInst, instrumentOptions)} />
      </ApplyToBar>

      {/* every sound that will be leveled — set any row directly, or tick for bulk */}
      <div
        style={{ flex: 1, minHeight: 0, overflowY: "auto", padding: "6px 0" }}
      >
        {groups.map((g) => (
          <div key={g.slot} style={{ padding: "10px 24px 12px" }}>
            <SetupGroupHeader slot={g.slot} name={g.name} />
            {g.opts.map((o) => {
              const tag = o.isBase ? (o.hasScenes ? "BASE" : null) : o.tag;
              const nameLabel = o.isBase ? "Whole preset" : o.sceneName;
              const sub = o.isBase
                ? "levels this preset against the others"
                : o.footswitch != null
                  ? "evens this footswitch out to your target"
                  : "levels this scene against the preset’s base";
              return (
                <PresetOptionRow
                  key={o.key}
                  name={nameLabel}
                  tag={tag ?? undefined}
                  isBase={o.isBase}
                  sub={sub}
                  isPicked={picked.has(o.key)}
                  onTogglePick={() => {
                    togglePick(o.key);
                  }}
                  title="Tick to bulk-edit this row with the bar above"
                  columns="132px 108px 108px"
                >
                  {/* Footswitch rows: choose which block parameter to level. Base +
                      scene rows keep the column empty so the pickers stay aligned. */}
                  {o.footswitch != null &&
                  o.levelParams &&
                  o.levelParams.length > 0 ? (
                    <FsParamPick
                      params={o.levelParams}
                      index={rowParam[o.key]}
                      onChange={(i) => {
                        setOneParam(o.key, i);
                      }}
                    />
                  ) : (
                    <div />
                  )}
                  <Pick
                    grow
                    value={rowInst[o.key] ?? defaultInst}
                    options={instrumentOptions}
                    onChange={(v) => {
                      setOneInst(o.key, v);
                    }}
                  />
                  <Pick
                    grow
                    tid={`target:${g.name}`}
                    value={rowTarget[o.key] ?? defaultTarget}
                    options={targetOptions}
                    onChange={(v) => {
                      setOneTarget(o.key, v);
                    }}
                  />
                </PresetOptionRow>
              );
            })}
          </div>
        ))}
      </div>

      {/* run option — advanced, opt-in, applies to the whole run. Mirrors the apply-to
          bar at the top (same tint + hairline) so the two config zones bookend the list.
          ALWAYS visible: the engine no-ops on non-merged sounds, and setup does no device
          reads (topology is only known once each preset loads at run time). */}
      <div
        style={{
          flexShrink: 0,
          padding: "12px 24px 13px",
          background: t.bgAlt,
          borderTop: `0.5px solid ${t.hairline}`,
        }}
      >
        <div style={{ ...s.kickerWide(t.faint), marginBottom: 9 }}>
          Run option
        </div>
        <div
          onClick={toggleRebalance}
          style={{
            display: "flex",
            alignItems: "flex-start",
            gap: 12,
            cursor: "pointer",
            userSelect: "none",
          }}
        >
          <span style={{ paddingTop: 1, flexShrink: 0 }}>
            <Toggle
              on={rebalance}
              onClick={(e) => {
                e.stopPropagation();
                toggleRebalance();
              }}
            />
          </span>
          <div style={{ minWidth: 0 }}>
            <div
              style={{
                fontFamily: t.sans,
                fontSize: 13,
                fontWeight: 500,
                color: t.ink,
              }}
            >
              Even out parallel amps
            </div>
            <div
              style={{
                fontFamily: t.sans,
                fontSize: 11,
                lineHeight: 1.5,
                color: t.mutedInk,
                marginTop: 2,
                textWrap: "pretty",
              }}
            >
              When a sound blends two amps into one, match their levels before
              leveling. No effect on single-amp sounds.
            </div>
            {rebalance && (
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 7,
                  marginTop: 7,
                }}
              >
                <ByEarChip />
                <span
                  style={{
                    fontFamily: t.sans,
                    fontSize: 11,
                    color: t.mutedInk,
                  }}
                >
                  Rebalanced sounds come back flagged for a listen.
                </span>
              </div>
            )}
          </div>
        </div>
      </div>

      <WizardFooter
        left={
          <Button
            variant="ghost"
            small
            onClick={onCancel}
            style={{ height: 32, padding: "0 15px" }}
          >
            Cancel
          </Button>
        }
        right={
          <>
            {requireBackup && (
              <BackupAckLabel
                checked={backedUp}
                onChange={setBackedUp}
                style={{ userSelect: "none", paddingRight: 4 }}
              />
            )}
            <Button
              variant="primary"
              small
              icon="gauge"
              disabled={total === 0 || (requireBackup && !backedUp)}
              onClick={start}
              style={{ height: 32, padding: "0 16px" }}
            >
              {`Level ${String(total)} sound${total === 1 ? "" : "s"}`}
            </Button>
          </>
        }
      />
    </>
  );
}

export default SetupBody;
