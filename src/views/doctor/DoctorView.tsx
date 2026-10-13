// src/views/doctor/DoctorView.tsx — the Doctor tab (tone diagnosis).
//
// A thin stage machine (select → setup → run → results) over the SAME expandable
// preset→scene tree as the Level tab. Each stage is its own component: only the
// Check (run) step is a modal; Set up and Results are full-page body swaps (the
// Copy-tab pattern). The select body stays MOUNTED under the run modal so the
// check reads as an overlay.
//
// The check is READ-ONLY on the device (a short test tone per sound); the ONLY
// writes are the results-page prescriptions (a later WP), each backup-gated +
// revertible.

import { useCallback, useMemo, useState } from "react";

import { LoadErrorPane } from "../LoadErrorPane";
import { EmptyState, UsbC } from "../EmptyState";
import { usePresetData } from "../level/usePresetData";
import { useLiveDevice } from "../../device/useLiveDevice";
import {
  chosenFrom,
  hasInstrument,
  instrumentOptions,
  instrumentName,
  type SetupOption,
} from "../level/leveling";
import { useDoctorFlow } from "./useDoctorFlow";
import { DoctorSelect } from "./DoctorSelect";
import { DoctorSetup } from "./DoctorSetup";
import { DoctorRun } from "./DoctorRun";
import { DoctorResults } from "./DoctorResults";
import type { PickOption } from "../overlays/Pick";

type Stage = "select" | "setup" | "run" | "results";

export interface DoctorViewProps {
  connected: boolean;
  onScan?: () => void;
}

export function DoctorView({ connected, onScan }: DoctorViewProps) {
  const {
    phase,
    rows,
    store,
    sel,
    pendingWhole,
    expanded,
    ready,
    presetCount,
    sceneCount,
    sceneInfo,
    footswitchInfo,
    allFootswitchInfo,
    silenceHintByIndex,
    graphByIndex,
    sceneOverridesByIndex,
    scan,
    togglePreset,
    toggleKey,
    toggleExpand,
    toggleAll,
    refresh,
  } = usePresetData(connected);

  const [stage, setStage] = useState<Stage>("select");
  const [filter, setFilter] = useState("");
  const [chosen, setChosen] = useState<SetupOption[]>([]);

  const flow = useDoctorFlow({
    store,
    graphByIndex,
    // The FULL footswitch roster (incl. Other-class-only switches) — the damage
    // detector's offline force-bypass derivation needs every switch that can toggle a
    // block, not just `footswitchInfo`'s levelable-only subset (that one still drives
    // the SELECT list below, where a non-levelable FS row has nothing to diagnose as
    // its own sound).
    footswitchesByIndex: allFootswitchInfo,
    sceneOverridesByIndex,
  });

  // The run recalls presets on the unit — remember the player's live slot so
  // the backend can restore it when the check ends (null → last-scanned slot).
  const { activeListIndex } = useLiveDevice(connected);

  // Instrument options — "None" + the store's profiles, calibrated ones flagged
  // (shared with the Level tab's Set up step). Drives every instrument Pick.
  const instOptions = useMemo<PickOption[]>(
    () => instrumentOptions(store?.profiles),
    [store?.profiles],
  );

  // The run's own items carry each sound's chosen profile — no second copy of
  // the setup step's picks is kept here.
  const runItems = flow.run.items;
  const instNameForKey = useCallback(
    (key: string): string | null => {
      const id = runItems.find((it) => it.key === key)?.profileId;
      if (id == null || !hasInstrument(id)) return null;
      return instrumentName(store?.profiles, id);
    },
    [runItems, store?.profiles],
  );

  const presetNames = useMemo(
    () => new Map(rows.map((r) => [r.slot, r.name])),
    [rows],
  );

  // Select → Set up: resolve the list's ticked scene keys into the setup rows.
  // ponytail: FS rows reuse the leveling filter (level_params-gated) — a
  // tone-changing FS with no levelable param won't appear; lift with a
  // diagnosis-specific chosenFrom if ever needed.
  const handleCheck = useCallback(() => {
    const options = chosenFrom(sel, rows, sceneInfo, footswitchInfo);
    if (options.length === 0) return;
    setChosen(options);
    setStage("setup");
  }, [sel, rows, sceneInfo, footswitchInfo]);

  // Set up → Run: build the wire items and fire the ONE check command.
  const { startRun, buildItems, reset } = flow;
  const handleRun = useCallback(
    (map: Record<string, string>) => {
      startRun(buildItems(chosen, map), activeListIndex);
      setStage("run");
    },
    [startRun, buildItems, chosen, activeListIndex],
  );

  // A failed run → the same sounds again.
  const handleRetry = useCallback(() => {
    startRun(runItems, activeListIndex);
  }, [startRun, runItems, activeListIndex]);

  // Results (or a failed run) → Select (fresh check on new sounds).
  const handleCheckMore = useCallback(() => {
    reset();
    setStage("select");
  }, [reset]);

  // ── render ──────────────────────────────────────────────────────────────────
  if (!connected) {
    return (
      <EmptyState
        title="Doctor needs the Tone Master Pro"
        body={
          <>
            Connect your unit over <UsbC /> and power it on to check your
            presets. It connects on its own.
          </>
        }
        onScan={onScan}
      />
    );
  }

  if (phase.kind === "error") {
    return (
      <LoadErrorPane message={phase.message} onRetry={() => void refresh()} />
    );
  }

  if (stage === "setup") {
    return (
      <DoctorSetup
        options={chosen}
        presetCount={new Set(chosen.map((o) => o.slot)).size}
        instrumentOptions={instOptions}
        store={store}
        onBack={() => {
          setStage("select");
        }}
        onRun={handleRun}
      />
    );
  }

  if (stage === "results" && flow.run.result) {
    // Each diagnosed sound's EXACT input (chain, full footswitch roster,
    // stimulus) — the prescription cards' A/B replays it, so the audition forces
    // off the same blocks the diagnosis did.
    const inputByKey = new Map(runItems.map((it) => [it.key, it]));
    return (
      <DoctorResults
        result={flow.run.result}
        presetNames={presetNames}
        inputByKey={inputByKey}
        silenceHintByIndex={silenceHintByIndex}
        onCheckMore={handleCheckMore}
      />
    );
  }

  return (
    <>
      <DoctorSelect
        rows={rows}
        sel={sel}
        pendingWhole={pendingWhole}
        expanded={expanded}
        ready={ready}
        filter={filter}
        loading={phase.kind === "loading"}
        scan={scan}
        sceneInfo={sceneInfo}
        footswitchInfo={footswitchInfo}
        presetCount={presetCount}
        sceneCount={sceneCount}
        onFilterChange={setFilter}
        onTogglePreset={togglePreset}
        onToggleExpand={toggleExpand}
        onToggleKey={toggleKey}
        onToggleAll={toggleAll}
        onCheck={handleCheck}
      />
      {stage === "run" && (
        <DoctorRun
          key={flow.run.attempt}
          status={flow.run.status}
          items={runItems}
          statusByKey={flow.run.statusByKey}
          error={flow.run.error}
          instName={instNameForKey}
          onStop={flow.stopRun}
          onComplete={() => {
            setStage("results");
          }}
          onRetry={handleRetry}
          onBack={handleCheckMore}
        />
      )}
    </>
  );
}
