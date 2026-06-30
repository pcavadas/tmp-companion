// src/views/overlays/wizardContext.ts — non-component shared bits of the leveling
// wizard: the dialog-card context, the stage union + rail-step type, and the
// stage→step mapping. Split from ./WizardShell so that file exports only
// components (React Fast Refresh's component-boundary rule) without disabling it.

export type Stage = "closed" | "setup" | "run" | "summary";

// DialogCardCtx now lives in the DS Dialog (every <Dialog> provides it); re-exported here
// so existing importers (Pick, FsParamPick, LevelSetupPage) keep their import path.
export { DialogCardCtx } from "../../ui/dialogContext";

/** One node on the header step rail. */
export interface RailStep {
  key: string;
  label: string;
}

/** The leveling wizard's 3-step rail, shared by the modal WizardShell (run/summary)
 *  and the full-page LevelSetupPage (set-up). */
export const WIZ_STEPS: readonly RailStep[] = [
  { key: "setup", label: "Set up" },
  { key: "level", label: "Level" },
  { key: "summary", label: "Summary" },
];

/** Stage → rail step index: setup 0 · run 1 · summary 2. */
export function stageToStep(stage: Stage): number {
  return stage === "setup" ? 0 : stage === "run" ? 1 : 2;
}
