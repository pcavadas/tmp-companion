// src/views/overlays/index.ts — barrel for the unified leveling WIZARD.
//
// Every stage (Set up · Level · Summary) is a full-page component in `../level/`
// (`SetupPage`/`RunPage`/`SummaryPage`, sharing `LevelPage`'s chrome) — LevelingWizard
// is the switch that routes stage → page. `WizardShell`/`WizardFooter`/`WizTitle`/
// `StepRail` stay exported here for Doctor and Bulk Block Edit, which still use the
// centered-modal wizard chrome.
export { LevelingWizard } from "./LevelingWizard";
export type { LevelingWizardProps } from "./LevelingWizard";
export { WizardShell, WizardFooter, WizTitle, StepRail } from "./WizardShell";
export type { Stage, WizardShellProps, WizardFooterProps } from "./WizardShell";
export { Pick } from "./Pick";
export type { PickOption, PickProps } from "./Pick";
