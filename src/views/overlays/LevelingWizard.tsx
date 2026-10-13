// src/views/overlays/LevelingWizard.tsx — routes props → the right full-page stage
// (design handoff 1a: full window for all three stages, replacing the old
// setup-is-full-page/run+summary-are-a-centered-modal split — `WizardShell`/
// `Dialog size="lg"` are no longer used by leveling; `WizardShell` itself stays for
// Doctor). Purely presentational: levelRunStore (bound through useLevelingFlow) owns
// the state machine and the device run.
//
// Stage → step: setup 0 (Set up) · run 1 (Level) · summary 2. Every stage is a
// full-bleed page over the Level tab's body (`LevelPage`, `zIndex:40`) with no
// backdrop at all — so Run can never be dismissed by a stray click mid-device-write,
// and Summary's primary Accept/Done button is the unconditional way out.

import type { Stage } from "./wizardContext";
import { SetupPage } from "../level/SetupPage";
import { RunPage } from "../level/RunPage";
import { SummaryPage } from "../level/SummaryPage";
import type { PickOption } from "./Pick";
import type { SetupOption, SetupChoice, RunItem } from "../level/leveling";
import type { RunState } from "../level/levelRunStore";

export interface LevelingWizardProps {
  stage: Exclude<Stage, "closed">;
  // setup inputs
  chosen: SetupOption[];
  isRelevel: boolean;
  instrumentOptions: PickOption[];
  targetOptions: PickOption[];
  defaultInst: string;
  defaultTarget: string;
  instrumentName: (id: string) => string;
  /** Resolve a target name to its LUFS (the run table's Target column). */
  targetLufsByName: (name: string | null) => number;
  /** The run's published state (the live meter is RunPage's own store read). */
  run: RunState;
  // callbacks
  onCancel: () => void;
  /** Set up's commit: the configured rows and its opt-in rebalance toggle. */
  onStart: (choices: SetupChoice[], rebalance: boolean) => void;
  onRunCancel: () => void;
  onRunComplete: () => void;
  onAccept: () => void;
  onRelevel: (subset: RunItem[]) => void;
  /** Jump to Settings → Instruments (the Set-up step's "calibrate" cue). */
  onCalibrate?: () => void;
}

export function LevelingWizard({
  stage,
  chosen,
  isRelevel,
  instrumentOptions,
  targetOptions,
  defaultInst,
  defaultTarget,
  instrumentName,
  targetLufsByName,
  run,
  onCancel,
  onStart,
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
        targetOptions={targetOptions}
        defaultInst={defaultInst}
        defaultTarget={defaultTarget}
        onCancel={onCancel}
        onStart={onStart}
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
