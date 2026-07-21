// src/views/overlays/LevelingWizard.tsx — routes props → the right body, and picks
// the body's CHROME by stage. Purely presentational: useLevelingFlow owns the state
// machine and the device run.
//
// Stage → step: setup 0 (Set up) · run 1 (Level) · summary 2.
// CONFIGURE phase (setup) renders FULL-PAGE (LevelSetupPage, replaces the Level body);
// WRITE phase (run/summary) stays a centered MODAL (WizardShell) — a device write is
// correctly blocking. Neither Run nor Summary can be dismissed by a stray backdrop
// click: Run because it must never abort an in-progress device operation, and Summary
// because it can carry actionable follow-ups (Re-level clamped…, Give clamped scenes
// headroom) that a stray click would otherwise silently discard with no confirmation —
// SummaryBody's primary Accept/Done button is unconditional across every branch, so the
// footer is always a reachable way out.

import { WizardShell } from "./WizardShell";
import { LevelSetupPage } from "./LevelSetupPage";
import { stageToStep, type Stage } from "./wizardContext";
import { SetupBody } from "./SetupBody";
import { RunBody } from "./RunBody";
import {
  SummaryBody,
  type RedistributionActions,
  type CommonTargetActions,
} from "./SummaryBody";
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
  /** Rolling per-hop momentary levels (dB) for the decorative live VU bars. */
  liveTrace: number[];
  // callbacks
  onCancel: () => void;
  onStart: (choices: SetupChoice[]) => void;
  onRunCancel: () => void;
  onRunComplete: () => void;
  onAccept: () => void;
  onRelevel: (clamped: RunItem[]) => void;
  redistribution?: RedistributionActions;
  commonTarget?: CommonTargetActions;
  onRebalanceChange?: (on: boolean) => void;
  /** Jump to Settings → Instruments (the Set-up step's "calibrate" cue). */
  onCalibrate?: () => void;
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
  liveTrace,
  onCancel,
  onStart,
  onRunCancel,
  onRunComplete,
  onAccept,
  onRelevel,
  redistribution,
  commonTarget,
  onRebalanceChange,
  onCalibrate,
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
          onCalibrate={onCalibrate}
        />
      </LevelSetupPage>
    );
  }

  // WRITE phase → centered modal (a device write is correctly blocking). Run and
  // Summary are both reached only here (setup returns early above) — neither takes
  // an onBackdrop, so the scrim is inert on both; see the file header for why.
  return (
    <WizardShell current={stageToStep(stage)}>
      {stage === "run" && (
        <RunBody
          items={runItems}
          currentIndex={runCurrentIndex}
          total={runTotal}
          done={runDone}
          stopped={runStopped}
          stopping={runStopping}
          liveLufs={liveLufs}
          liveTrace={liveTrace}
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
          redistribution={redistribution}
          commonTarget={commonTarget}
        />
      )}
    </WizardShell>
  );
}

export default LevelingWizard;
