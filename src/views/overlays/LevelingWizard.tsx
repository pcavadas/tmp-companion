// src/views/overlays/LevelingWizard.tsx — the leveling flow's stage switch. Purely
// presentational: levelRunStore (bound through useLevelingFlow) owns the state machine
// and the device run.
//
// Every stage is a look B page in the Level tab's content area, under the toolbar
// (`FlowPage`): Set up (`SetupPage`), the run (`RunPage`, whose Stop asks first in a
// dialog that a stray click can't dismiss) and the Summary (`SummaryPage`).

import type { Stage } from "./wizardContext";
import { SetupPage } from "../level/SetupPage";
import { RunPage } from "../level/RunPage";
import { SummaryPage } from "../level/SummaryPage";
import type { SelectOption } from "../../ui/selectValue";
import type {
  SetupOption,
  SetupChoice,
  RunItem,
  TargetLufsOf,
} from "../level/leveling";
import type { RunState, SummaryScenes } from "../level/levelRunStore";

export interface LevelingWizardProps {
  stage: Exclude<Stage, "closed">;
  // setup inputs
  chosen: SetupOption[];
  /** "None" + each instrument (`value` = profile id). */
  instrumentOptions: SelectOption[];
  /** The instruments with a recording. */
  recordedInstruments: ReadonlySet<string>;
  /** The configured targets' names, in their order. */
  targetNames: string[];
  /** The instrument saved for each preset (0-based list index → id). */
  savedInstruments: Partial<Record<number, string>>;
  defaultInst: string;
  /** Resolve a target name to its LUFS (the run's meter, the Summary's clamp side). */
  targetLufsByName: TargetLufsOf;
  /** The run on screen (the live meter is RunPage's own store read). */
  run: RunState;
  /** Every sound the flow ran, with its latest result (the Summary's rows). */
  results: RunItem[];
  /** Footswitch scenes changed on the Summary. */
  summaryScenes: SummaryScenes;
  /** Re-amp may still be on, by the flow's latest reading (the Summary's Notice). */
  reampStillOn: boolean;
  // callbacks
  onCancel: () => void;
  /** Set up's commit: the configured sounds and the presets balanced first. */
  onStart: (choices: SetupChoice[], rebalance: number[]) => void;
  /** Remember a preset's instrument choice. */
  onInstrumentChange?: (slot: number, id: string) => void;
  onRunCancel: () => void;
  onRunComplete: () => void;
  onSummaryScene: (key: string, scene: number | null | undefined) => void;
  onTryAgain: (keys: readonly string[]) => void;
  onDone: () => void;
  /** Jump to Settings → Instruments ("Record yours in Settings"). */
  onCalibrate?: () => void;
  /** Jump to Settings → Loudness targets ("Add a target in Settings"). */
  onAddTarget?: () => void;
  /** The app's store is still loading (Set up waits for its targets). */
  storeLoading?: boolean;
}

export function LevelingWizard({
  stage,
  chosen,
  instrumentOptions,
  recordedInstruments,
  targetNames,
  savedInstruments,
  defaultInst,
  targetLufsByName,
  run,
  results,
  summaryScenes,
  reampStillOn,
  onCancel,
  onStart,
  onInstrumentChange,
  onRunCancel,
  onRunComplete,
  onSummaryScene,
  onTryAgain,
  onDone,
  onCalibrate,
  onAddTarget,
  storeLoading,
}: LevelingWizardProps) {
  if (stage === "setup") {
    return (
      <SetupPage
        options={chosen}
        instrumentOptions={instrumentOptions}
        recordedInstruments={recordedInstruments}
        targetNames={targetNames}
        savedInstruments={savedInstruments}
        defaultInst={defaultInst}
        onCancel={onCancel}
        onStart={onStart}
        onInstrumentChange={onInstrumentChange}
        onCalibrate={onCalibrate}
        onAddTarget={onAddTarget}
        storeLoading={storeLoading}
      />
    );
  }

  if (stage === "run") {
    return (
      <RunPage
        items={run.items}
        currentIndex={run.currentIndex}
        total={run.total}
        phase={run.phase}
        tailMessage={run.tailMessage}
        targetLufsByName={targetLufsByName}
        onStop={onRunCancel}
        onComplete={onRunComplete}
      />
    );
  }

  return (
    <SummaryPage
      items={results}
      stopped={run.phase === "stopped"}
      reampStillOn={reampStillOn}
      targetLufsByName={targetLufsByName}
      sceneOverrides={summaryScenes}
      onSceneChange={onSummaryScene}
      onTryAgain={onTryAgain}
      onDone={onDone}
    />
  );
}
