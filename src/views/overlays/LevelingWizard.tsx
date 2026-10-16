// src/views/overlays/LevelingWizard.tsx — the leveling flow's stage switch. Purely
// presentational: levelRunStore (bound through useLevelingFlow) owns the state machine
// and the device run.
//
// Set up is a page in the Level tab's content area (look B: `SetupPage`, under the
// toolbar, no overlay). The run and the Summary still draw over the Level tab's body on
// the full-bleed `LevelPage` (`zIndex: 40`, no backdrop at all — so the run can never be
// dismissed by a stray click mid-device-write) until they become pages too.

import type { Stage } from "./wizardContext";
import { SetupPage } from "../level/SetupPage";
import { RunPage } from "../level/RunPage";
import { SummaryPage } from "../level/SummaryPage";
import type { SelectOption } from "../../ui/selectValue";
import type { SetupOption, SetupChoice, RunItem } from "../level/leveling";
import type { RunState } from "../level/levelRunStore";

export interface LevelingWizardProps {
  stage: Exclude<Stage, "closed">;
  // setup inputs
  chosen: SetupOption[];
  isRelevel: boolean;
  /** "None" + each instrument (`value` = profile id). */
  instrumentOptions: SelectOption[];
  /** The instruments with a recording. */
  recordedInstruments: ReadonlySet<string>;
  /** The configured targets' names, in their order. */
  targetNames: string[];
  /** The instrument saved for each preset (0-based list index → id). */
  savedInstruments: Partial<Record<number, string>>;
  defaultInst: string;
  instrumentName: (id: string) => string;
  /** Resolve a target name to its LUFS (the run table's Target column). */
  targetLufsByName: (name: string | null) => number;
  /** The run's published state (the live meter is RunPage's own store read). */
  run: RunState;
  // callbacks
  onCancel: () => void;
  /** Set up's commit: the configured sounds and the presets balanced first. */
  onStart: (choices: SetupChoice[], rebalance: number[]) => void;
  /** Remember a preset's instrument choice. */
  onInstrumentChange?: (slot: number, id: string) => void;
  onRunCancel: () => void;
  onRunComplete: () => void;
  onAccept: () => void;
  onRelevel: (subset: RunItem[]) => void;
  /** Jump to Settings → Instruments ("Record yours in Settings"). */
  onCalibrate?: () => void;
}

export function LevelingWizard({
  stage,
  chosen,
  isRelevel,
  instrumentOptions,
  recordedInstruments,
  targetNames,
  savedInstruments,
  defaultInst,
  instrumentName,
  targetLufsByName,
  run,
  onCancel,
  onStart,
  onInstrumentChange,
  onRunCancel,
  onRunComplete,
  onAccept,
  onRelevel,
  onCalibrate,
}: LevelingWizardProps) {
  if (stage === "setup") {
    return (
      <SetupPage
        options={chosen}
        isRelevel={isRelevel}
        instrumentOptions={instrumentOptions}
        recordedInstruments={recordedInstruments}
        targetNames={targetNames}
        savedInstruments={savedInstruments}
        defaultInst={defaultInst}
        onCancel={onCancel}
        onStart={onStart}
        onInstrumentChange={onInstrumentChange}
        onCalibrate={onCalibrate}
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
        instrumentName={instrumentName}
        targetLufsByName={targetLufsByName}
        onCancel={onRunCancel}
        onComplete={onRunComplete}
      />
    );
  }

  return (
    <SummaryPage
      items={run.items}
      stopped={run.phase === "stopped"}
      onAccept={onAccept}
      onRelevel={onRelevel}
    />
  );
}
