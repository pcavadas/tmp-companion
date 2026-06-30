// src/views/overlays/index.ts — barrel for the unified leveling WIZARD.
//
// One persistent frame (WizardShell) with a 3-step rail (Set up · Level · Summary)
// whose body swaps per stage. LevelingWizard is the switch; the three bodies + the
// Pick dropdown are the parts.
export { LevelingWizard } from "./LevelingWizard";
export type { LevelingWizardProps } from "./LevelingWizard";
export { WizardShell, WizardFooter, WizTitle } from "./WizardShell";
export type { Stage, WizardShellProps, WizardFooterProps } from "./WizardShell";
export { stageToStep } from "./wizardContext";
export { SetupBody } from "./SetupBody";
export type { SetupBodyProps, SetupChoice } from "./SetupBody";
export { RunBody } from "./RunBody";
export type { RunBodyProps } from "./RunBody";
export { SummaryBody } from "./SummaryBody";
export type { SummaryBodyProps } from "./SummaryBody";
export { Pick } from "./Pick";
export type { PickOption, PickProps } from "./Pick";
