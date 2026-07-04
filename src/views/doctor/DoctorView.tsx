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
import {
  chosenFrom,
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

// Doctor measures a preset's BASE and its scenes — the check wire has no
// engaged-footswitch state, so footswitch rows are excluded from this tab's
// tree. A frozen module-level empty map (mirroring `NO_CPU` in DoctorSelect)
// keeps a stable identity without a per-mount allocation.
const NO_FOOTSWITCHES = new Map<number, never[]>();

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
    graphByIndex,
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
  const [instByKey, setInstByKey] = useState<Record<string, string>>({});

  const flow = useDoctorFlow({ store, graphByIndex });

  // Instrument options — "None" + the store's profiles, calibrated ones flagged
  // (shared with the Level tab's Set up step). Drives every instrument Pick.
  const instOptions = useMemo<PickOption[]>(
    () => instrumentOptions(store?.profiles),
    [store?.profiles],
  );

  const instNameForKey = useCallback(
    (key: string): string | null => {
      const id = instByKey[key];
      if (!id || id === "none") return null;
      return instrumentName(store?.profiles, id);
    },
    [instByKey, store?.profiles],
  );

  const presetNames = useMemo(
    () => new Map(rows.map((r) => [r.slot, r.name])),
    [rows],
  );

  // Select → Set up: resolve the list's ticked scene keys into the setup rows.
  const handleCheck = useCallback(() => {
    const options = chosenFrom(sel, rows, sceneInfo, NO_FOOTSWITCHES);
    if (options.length === 0) return;
    setChosen(options);
    setStage("setup");
  }, [sel, rows, sceneInfo]);

  // Set up → Run: build the wire items and fire the ONE check command.
  const handleRun = useCallback(
    (map: Record<string, string>) => {
      setInstByKey(map);
      flow.startRun(flow.buildItems(chosen, map));
      setStage("run");
    },
    [flow, chosen],
  );

  // Results → Select (fresh check on new sounds).
  const handleCheckMore = useCallback(() => {
    flow.reset();
    setStage("select");
  }, [flow]);

  // ── render ──────────────────────────────────────────────────────────────────
  if (!connected) {
    return (
      <EmptyState
        title="Doctor needs the Tone Master Pro"
        body={
          <>
            Connect your unit over <UsbC /> and power it on to run diagnostics —
            it will connect automatically.
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

  if (stage === "results" && flow.result) {
    return (
      <DoctorResults
        result={flow.result}
        presetNames={presetNames}
        onCheckMore={handleCheckMore}
      />
    );
  }

  // The run rejected outright (backend/IPC failure) — no results to show. Surface
  // it instead of silently dropping back to the list (a failed run otherwise
  // looks identical to an intentional Stop).
  if (stage === "results" && flow.error) {
    return (
      <LoadErrorPane
        message={`The check couldn't finish: ${flow.error}`}
        onRetry={handleCheckMore}
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
        footswitchInfo={NO_FOOTSWITCHES}
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
          items={flow.run.items}
          statusByKey={flow.run.statusByKey}
          currentIndex={flow.run.currentIndex}
          total={flow.run.total}
          done={flow.run.done}
          stopped={flow.run.stopped}
          instName={instNameForKey}
          onStop={flow.stopRun}
          onComplete={() => {
            setStage("results");
          }}
        />
      )}
    </>
  );
}

export default DoctorView;
