// src/views/overlays/LevelingWizard.tsx — routes props → the right body, and picks
// the body's CHROME by stage. Purely presentational: useLevelingFlow owns the state
// machine and the device run.
//
// Stage → step: setup 0 (Set up) · run 1 (Level) · summary 2.
// CONFIGURE phase (setup) renders FULL-PAGE (LevelSetupPage, replaces the Level body);
// WRITE phase (run/summary) stays a centered MODAL (WizardShell) — a device write is
// correctly blocking. Backdrop click closes every modal stage EXCEPT run (never abort a
// device operation).

import { WizardShell } from "./WizardShell";
import { LevelSetupPage } from "./LevelSetupPage";
import { stageToStep, type Stage } from "./wizardContext";
import { SetupBody } from "./SetupBody";
import { RunBody } from "./RunBody";
import { SummaryBody } from "./SummaryBody";
import type { PickOption } from "./Pick";
import type { SetupOption, SetupChoice, RunItem } from "../level/leveling";

export interface LevelingWizardProps {
  stage: Exclude<Stage, "closed">;
  // setup inputs
  chosen: SetupOption[];
  flowPresetCount: number;
  isRelevel: boolean;
  instrumentOptions: PickOption[];
  targetOptions: PickOption[];
  defaultInst: string;
  defaultTarget: string;
  instrumentName: (id: string) => string;
  // run state
  runItems: RunItem[];
  runCurrentIndex: number;
  runTotal: number;
  runDone: boolean;
  runStopped: boolean;
  runStopping: boolean;
  /** Advisory live measured loudness for the active run row (null = nothing measuring). */
  liveLufs: number | null;
  // callbacks
  onCancel: () => void;
  onStart: (choices: SetupChoice[]) => void;
  onRunCancel: () => void;
  onRunComplete: () => void;
  onAccept: () => void;
  onRelevel: (clamped: RunItem[]) => void;
  onRebalanceChange?: (on: boolean) => void;
}

export function LevelingWizard({
  stage,
  chosen,
  flowPresetCount,
  isRelevel,
  instrumentOptions,
  targetOptions,
  defaultInst,
  defaultTarget,
  instrumentName,
  runItems,
  runCurrentIndex,
  runTotal,
  runDone,
  runStopped,
  runStopping,
  liveLufs,
  onCancel,
  onStart,
  onRunCancel,
  onRunComplete,
  onAccept,
  onRelevel,
  onRebalanceChange,
}: LevelingWizardProps) {
  // CONFIGURE phase → full-page page that replaces the Level body.
  if (stage === "setup") {
    return (
      <LevelSetupPage stage={stage}>
        <SetupBody
          options={chosen}
          presetCount={flowPresetCount}
          isRelevel={isRelevel}
          instrumentOptions={instrumentOptions}
          targetOptions={targetOptions}
          defaultInst={defaultInst}
          defaultTarget={defaultTarget}
          onCancel={onCancel}
          onStart={onStart}
          onRebalanceChange={onRebalanceChange}
        />
      </LevelSetupPage>
    );
  }

  // WRITE phase → centered modal (a device write is correctly blocking).
  return (
    <WizardShell
      current={stageToStep(stage)}
      onBackdrop={stage === "run" ? undefined : onCancel}
    >
      {stage === "run" && (
        <RunBody
          items={runItems}
          currentIndex={runCurrentIndex}
          total={runTotal}
          done={runDone}
          stopped={runStopped}
          stopping={runStopping}
          liveLufs={liveLufs}
          instrumentName={instrumentName}
          onCancel={onRunCancel}
          onComplete={onRunComplete}
        />
      )}
      {stage === "summary" && (
        <SummaryBody
          items={runItems}
          stopped={runStopped}
          onAccept={onAccept}
          onRelevel={onRelevel}
        />
      )}
    </WizardShell>
  );
}

export default LevelingWizard;
