// src/views/overlays/LevelingWizard.tsx — routes props → the right full-page stage
// (design handoff 1a: full window for all three stages, replacing the old
// setup-is-full-page/run+summary-are-a-centered-modal split — `WizardShell`/
// `Dialog size="lg"` are no longer used by leveling; `WizardShell` itself stays for
// Doctor). Purely presentational: useLevelingFlow owns the state machine and the
// device run.
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
  // run state
  runItems: RunItem[];
  runCurrentIndex: number;
  runTotal: number;
  runDone: boolean;
  runStopped: boolean;
  runStopping: boolean;
  /** Advisory live measured loudness for the active run row (null = nothing measuring). */
  liveLufs: number | null;
  /** Rolling per-hop momentary levels (dB) for the decorative live VU bars. */
  liveTrace: number[];
  /** A batch-wide caption (issue 6b) — see `RunPage`'s own doc. */
  runTailMessage?: string | null;
  // callbacks
  onCancel: () => void;
  onStart: (choices: SetupChoice[]) => void;
  onRunCancel: () => void;
  onRunComplete: () => void;
  onAccept: () => void;
  onRelevel: (subset: RunItem[]) => void;
  onRebalanceChange?: (on: boolean) => void;
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
  runItems,
  runCurrentIndex,
  runTotal,
  runDone,
  runStopped,
  runStopping,
  liveLufs,
  liveTrace,
  runTailMessage,
  onCancel,
  onStart,
  onRunCancel,
  onRunComplete,
  onAccept,
  onRelevel,
  onRebalanceChange,
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
        onRebalanceChange={onRebalanceChange}
        onCalibrate={onCalibrate}
      />
    );
  }

  if (stage === "run") {
    return (
      <RunPage
        items={runItems}
        currentIndex={runCurrentIndex}
        total={runTotal}
        done={runDone}
        stopped={runStopped}
        stopping={runStopping}
        liveLufs={liveLufs}
        liveTrace={liveTrace}
        tailMessage={runTailMessage}
        instrumentName={instrumentName}
        targetLufsByName={targetLufsByName}
        onCancel={onRunCancel}
        onComplete={onRunComplete}
      />
    );
  }

  return (
    <SummaryPage
      items={runItems}
      stopped={runStopped}
      onAccept={onAccept}
      onRelevel={onRelevel}
    />
  );
}

export default LevelingWizard;
