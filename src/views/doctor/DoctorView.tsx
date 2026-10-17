// src/views/doctor/DoctorView.tsx — the Doctor tab (tone diagnosis), look B.
//
// A small stage machine over four pages under the toolbar: choose the sounds on the shared
// preset list (DoctorSelect) → which instrument plays them (DoctorInstrument, skipped when
// every chosen preset already has one saved) → the check (DoctorRun) → Results
// (DoctorResults). Each is a full page; the list is not kept under the others.
//
// Doctor only listens and describes: the check plays a short test tone through each sound
// and changes nothing on the unit, and every fix is a step the player makes by hand. The
// one thing it remembers is each preset's instrument, in the app's own store on this
// computer (never on the unit).

import { useCallback, useMemo, useState } from "react";

import { LoadErrorPane } from "../LoadErrorPane";
import { DeviceState } from "../DeviceState";
import type { NoUnitState } from "../../lib/connectError";
import { usePresetData } from "../level/usePresetData";
import { useLiveDevice } from "../../device/useLiveDevice";
import {
  chosenFrom,
  instrumentOptions,
  type SetupOption,
} from "../level/leveling";
import {
  useDoctorFlow,
  DOCTOR_STEPS,
  DOCTOR_STEPS_SKIPPED,
} from "./useDoctorFlow";
import { DoctorSelect } from "./DoctorSelect";
import { DoctorInstrument } from "./DoctorInstrument";
import { DoctorRun } from "./DoctorRun";
import { DoctorResults } from "./DoctorResults";
import {
  everyInstrumentChosen,
  initialInstruments,
  presetsOf,
} from "./instrumentModel";
import {
  soundLabel,
  soundNameOf,
  type ResultsContext,
  type SoundName,
} from "./resultsModel";
import type { RunName } from "./runRows";

type Stage = "select" | "instrument" | "run" | "results";

export interface DoctorViewProps {
  connected: boolean;
  /** What to show while no unit is connected (App's connect state). Default "off". */
  deviceState?: NoUnitState;
  /** "Record yours in Settings": Settings › Instruments. */
  onCalibrate?: () => void;
}

export function DoctorView({
  connected,
  deviceState = "off",
  onCalibrate,
}: DoctorViewProps) {
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
    rememberInstrument,
  } = usePresetData(connected);

  const [stage, setStage] = useState<Stage>("select");
  const [filter, setFilter] = useState("");
  const [chosen, setChosen] = useState<SetupOption[]>([]);
  // The Instrument page was skipped: every chosen preset's instrument was known.
  const [skipped, setSkipped] = useState(false);

  const flow = useDoctorFlow({
    store,
    graphByIndex,
    // The FULL footswitch roster (incl. Other-class-only switches) — the force-bypass
    // derivation needs every switch that can toggle a block, not just `footswitchInfo`'s
    // levelable-only subset (that one still drives the list, where a non-levelable
    // switch has nothing to diagnose as its own sound).
    footswitchesByIndex: allFootswitchInfo,
    sceneOverridesByIndex,
  });

  // The run recalls presets on the unit — remember the player's live slot so the backend
  // can put it back when the check ends; it also marks "Loaded" on the list.
  const { activeListIndex } = useLiveDevice(connected);

  const instOptions = useMemo(
    () => instrumentOptions(store?.profiles),
    [store?.profiles],
  );
  const recordedInstruments = useMemo<ReadonlySet<string>>(
    () =>
      new Set(
        (store?.profiles ?? [])
          .filter((p) => p.calibration_lufs != null)
          .map((p) => p.id),
      ),
    [store?.profiles],
  );

  const presets = useMemo(() => presetsOf(chosen), [chosen]);
  const [instInit, setInstInit] = useState<ReadonlyMap<number, string>>(
    () => new Map(),
  );

  // How each chosen sound is named on the Check and Results pages.
  const soundNames = useMemo(
    () =>
      new Map<string, SoundName>(chosen.map((o) => [o.key, soundNameOf(o)])),
    [chosen],
  );
  const presetNames = useMemo(
    () => new Map(rows.map((r) => [r.slot, r.name])),
    [rows],
  );
  const resultsCtx = useMemo<ResultsContext>(
    () => ({ soundNames, presetNames, silenceHintByIndex }),
    [soundNames, presetNames, silenceHintByIndex],
  );
  const nameOf = useCallback(
    (key: string): RunName => {
      const n = soundNames.get(key);
      return n
        ? { preset: n.preset, sound: soundLabel(n) }
        : { preset: "", sound: key };
    },
    [soundNames],
  );

  const runItems = flow.run.items;
  const { startRun, buildItems, reset } = flow;

  // Select → (Instrument →) Check. The Instrument page is skipped when every chosen
  // preset already has an instrument saved (an explicit "None" counts).
  const handleCheck = useCallback(() => {
    const options = chosenFrom(sel, rows, sceneInfo, footswitchInfo);
    if (options.length === 0) return;
    const init = initialInstruments(presetsOf(options), store, instOptions);
    setChosen(options);
    if (everyInstrumentChosen(init)) {
      setSkipped(true);
      startRun(buildItems(options, init), activeListIndex);
      setStage("run");
    } else {
      setSkipped(false);
      setInstInit(init);
      setStage("instrument");
    }
  }, [
    sel,
    rows,
    sceneInfo,
    footswitchInfo,
    store,
    instOptions,
    startRun,
    buildItems,
    activeListIndex,
  ]);

  const handleRun = useCallback(
    (instBySlot: ReadonlyMap<number, string>) => {
      startRun(buildItems(chosen, instBySlot), activeListIndex);
      setStage("run");
    },
    [startRun, buildItems, chosen, activeListIndex],
  );

  // The same sounds again, with the same instruments (a failed check, or Results).
  const handleAgain = useCallback(() => {
    startRun(runItems, activeListIndex);
    setStage("run");
  }, [startRun, runItems, activeListIndex]);

  // Back to the list, ticks kept.
  const handleCheckMore = useCallback(() => {
    reset();
    setStage("select");
  }, [reset]);

  const inputByKey = useMemo(
    () => new Map(runItems.map((it) => [it.key, it])),
    [runItems],
  );
  const steps = skipped ? DOCTOR_STEPS_SKIPPED : DOCTOR_STEPS;

  // ── render ──────────────────────────────────────────────────────────────────
  if (!connected) return <DeviceState state={deviceState} />;

  if (phase.kind === "error") {
    return <LoadErrorPane what="presets" onRetry={() => void refresh()} />;
  }

  if (stage === "instrument") {
    return (
      <DoctorInstrument
        presets={presets}
        soundCount={chosen.length}
        instrumentOptions={instOptions}
        recordedInstruments={recordedInstruments}
        initial={instInit}
        onInstrumentChange={rememberInstrument}
        onBack={() => {
          setStage("select");
        }}
        onRun={handleRun}
        onCalibrate={onCalibrate}
      />
    );
  }

  if (stage === "run") {
    return (
      <DoctorRun
        key={flow.run.attempt}
        status={flow.run.status}
        items={runItems}
        statusByKey={flow.run.statusByKey}
        nameOf={nameOf}
        steps={steps}
        onStop={flow.stopRun}
        onComplete={() => {
          setStage("results");
        }}
        onRetry={handleAgain}
        onBack={handleCheckMore}
      />
    );
  }

  if (stage === "results" && flow.run.result) {
    return (
      <DoctorResults
        key={flow.run.attempt}
        result={flow.run.result}
        ctx={resultsCtx}
        inputByKey={inputByKey}
        steps={steps}
        onCheckMore={handleCheckMore}
        onCheckAgain={handleAgain}
      />
    );
  }

  return (
    <DoctorSelect
      rows={rows}
      sel={sel}
      pendingWhole={pendingWhole}
      expanded={expanded}
      ready={ready}
      activeSlot={activeListIndex}
      filter={filter}
      loading={phase.kind === "loading"}
      scan={scan}
      sceneInfo={sceneInfo}
      footswitchInfo={footswitchInfo}
      presetCount={presetCount}
      soundCount={sceneCount}
      onFilterChange={setFilter}
      onTogglePreset={togglePreset}
      onToggleExpand={toggleExpand}
      onToggleKey={toggleKey}
      onToggleAll={toggleAll}
      onCheck={handleCheck}
    />
  );
}
