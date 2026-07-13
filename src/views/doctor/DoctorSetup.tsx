// src/views/doctor/DoctorSetup.tsx — the Doctor "Set up" page (full-page body swap).
//
// Doctor listens differently for a bass than a bright single-coil, so this step
// sets the INSTRUMENT context per sound before the check. Mirrors the leveling
// SetupBody's apply-to brush (bulk Pick + per-row tick scoping + "Clear ticks")
// and its full-page chrome (LevelSetupPage): the page provides its OWN ref as
// DialogCardCtx so the Pick dropdowns portal into THIS page's coordinate space.
//
// Everything picked in the list WILL be checked — this step never re-gates it.
// Per-row default = the slot's saved profile → the last-used instrument → None.

import { useMemo, useRef, useState } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Button } from "../../ui/primitives";
import { SetupGroupHeader } from "../../ui/SetupGroupHeader";
import { PresetOptionRow } from "../../ui/PresetOptionRow";
import { ApplyToBar } from "../../ui/ApplyToBar";
import { usePickedRows } from "../../lib/usePickedRows";
import { StepRail, WizardFooter, WizTitle } from "../overlays/WizardShell";
import { DialogCardCtx } from "../overlays/wizardContext";
import { Pick, type PickOption } from "../overlays/Pick";
import { DOCTOR_STEPS } from "./useDoctorFlow";
import type { SetupOption } from "../level/leveling";
import type { Store } from "../../lib/types";

/** localStorage key for the app-wide last-used Doctor instrument. */
const LAST_INST_KEY = "tmp_doctor_last_inst";

/** Per-row instrument default: the slot's saved profile, else the last-used
 *  instrument, else "none" — but only ids that still exist in the options. */
function defaultInstFor(
  o: SetupOption,
  store: Store | null,
  options: PickOption[],
): string {
  const has = (id: string | undefined | null): id is string =>
    id != null && options.some((op) => op.id === id);
  const bySlot = store?.profile_by_slot[o.slot];
  if (has(bySlot)) return bySlot;
  const last = localStorage.getItem(LAST_INST_KEY);
  if (has(last)) return last;
  return "none";
}

export interface DoctorSetupProps {
  /** The exact sounds picked in the list — all will be checked. */
  options: SetupOption[];
  /** How many presets the check spans (for the sub-line). */
  presetCount: number;
  /** "None" + the store's instrument profiles (calibrated ones flagged). */
  instrumentOptions: PickOption[];
  store: Store | null;
  onBack: () => void;
  /** Commit → the per-row instrument map (key → profile id / "none"). */
  onRun: (instByKey: Record<string, string>) => void;
}

export function DoctorSetup({
  options,
  presetCount,
  instrumentOptions,
  store,
  onBack,
  onRun,
}: DoctorSetupProps) {
  const { t } = useTheme();
  const pageRef = useRef<HTMLDivElement>(null);

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

  // Per-row instrument — the real value that gets checked.
  const [rowInst, setRowInst] = useState<Record<string, string>>(() => {
    const m: Record<string, string> = {};
    options.forEach(
      (o) => (m[o.key] = defaultInstFor(o, store, instrumentOptions)),
    );
    return m;
  });
  const rememberLast = (id: string) => {
    if (id !== "none") localStorage.setItem(LAST_INST_KEY, id);
  };
  const setOneInst = (k: string, v: string) => {
    setRowInst((p) => ({ ...p, [k]: v }));
    rememberLast(v);
  };

  // Bulk-edit tick selection (which rows the "Apply to" bar writes to). Empty = all.
  const {
    picked,
    togglePick,
    clearPicked,
    somePicked,
    targetsForBulk,
    scopeLabel,
  } = usePickedRows(options);

  const [bulkInst, setBulkInst] = useState("none");
  const applyBulkInst = (v: string) => {
    setBulkInst(v);
    rememberLast(v);
    setRowInst((p) => {
      const n = { ...p };
      targetsForBulk().forEach((k) => (n[k] = v));
      return n;
    });
  };

  const total = options.length;
  const anyNone = options.some((o) => (rowInst[o.key] ?? "none") === "none");

  const run = () => {
    const instByKey: Record<string, string> = {};
    options.forEach((o) => (instByKey[o.key] = rowInst[o.key] ?? "none"));
    if (total > 0) onRun(instByKey);
  };

  return (
    <div
      ref={pageRef}
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
      {/* slim step-rail header — Back link + the Doctor rail (current 0) */}
      <div
        style={{
          flexShrink: 0,
          display: "flex",
          alignItems: "center",
          gap: t.space8,
          padding: `${String(t.space6)}px ${String(t.space10)}px`,
          borderBottom: `0.5px solid ${t.hairline}`,
          background: t.bgAlt,
        }}
      >
        <span
          onClick={onBack}
          style={{
            fontFamily: t.sans,
            fontSize: t.fsLabel,
            color: t.mutedInk,
            cursor: "pointer",
            whiteSpace: "nowrap",
            flexShrink: 0,
          }}
        >
          Back
        </span>
        <div style={{ flex: 1, minWidth: 0 }}>
          <StepRail current={0} steps={DOCTOR_STEPS} />
        </div>
      </div>

      <DialogCardCtx.Provider value={pageRef}>
        {/* title block */}
        <div
          style={{
            flexShrink: 0,
            padding: `${String(t.space8)}px ${String(t.space10)}px ${String(t.space7)}px`,
            borderBottom: `0.5px solid ${t.hairline}`,
          }}
        >
          <WizTitle>What are you playing?</WizTitle>
          <div
            style={{
              fontFamily: t.sans,
              fontSize: t.fsBody2,
              lineHeight: 1.5,
              color: t.mutedInk,
              marginTop: t.space4,
              maxWidth: 620,
            }}
          >
            Doctor listens differently for a bass than a bright single-coil.
            Pick the instrument for each sound — {total} sound
            {total === 1 ? "" : "s"} · {presetCount} preset
            {presetCount === 1 ? "" : "s"}.
          </div>
        </div>

        {/* apply-to bar — writes to all rows, or to the ticked rows */}
        <ApplyToBar
          label={`Instrument for ${scopeLabel}`}
          somePicked={somePicked}
          onClear={clearPicked}
        >
          <div style={{ maxWidth: 260 }}>
            <Pick
              grow
              value={bulkInst}
              options={instrumentOptions}
              onChange={applyBulkInst}
            />
          </div>
        </ApplyToBar>

        {/* every sound that will be checked — set any row directly, or tick for bulk */}
        <div
          style={{
            flex: 1,
            minHeight: 0,
            overflowY: "auto",
            padding: `${String(t.space3)}px 0`,
          }}
        >
          {groups.map((g) => (
            <div
              key={g.slot}
              style={{
                padding: `${String(t.space5)}px ${String(t.space10)}px ${String(t.space6)}px`,
              }}
            >
              <SetupGroupHeader slot={g.slot} name={g.name} />
              {g.opts.map((o) => {
                const tag = o.isBase ? (o.hasScenes ? "BASE" : null) : o.tag;
                const nameLabel = o.isBase ? "Whole preset" : o.sceneName;
                return (
                  <PresetOptionRow
                    key={o.key}
                    name={nameLabel}
                    tag={tag ?? undefined}
                    isBase={o.isBase}
                    isPicked={picked.has(o.key)}
                    onTogglePick={() => {
                      togglePick(o.key);
                    }}
                    title="Tick to bulk-set this row with the bar above"
                    columns="200px"
                  >
                    <Pick
                      grow
                      tid={`inst:${o.key}`}
                      value={rowInst[o.key] ?? "none"}
                      options={instrumentOptions}
                      onChange={(v) => {
                        setOneInst(o.key, v);
                      }}
                    />
                  </PresetOptionRow>
                );
              })}
            </div>
          ))}
        </div>

        <WizardFooter
          left={
            <span
              style={{
                fontFamily: t.sans,
                fontSize: t.fsBody2,
                color: anyNone ? t.mutedInk : t.good,
              }}
            >
              {anyNone
                ? "Set an instrument for the most accurate check."
                : `Ready to check ${String(total)} sound${total === 1 ? "" : "s"}.`}
            </span>
          }
          right={
            <Button
              variant="primary"
              small
              icon="wave"
              disabled={total === 0}
              onClick={run}
              style={{ height: 32, padding: `0 ${String(t.space8)}px` }}
            >
              {`Run check on ${String(total)} sound${total === 1 ? "" : "s"}`}
            </Button>
          }
        />
      </DialogCardCtx.Provider>
    </div>
  );
}

export default DoctorSetup;
