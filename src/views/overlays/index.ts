// src/views/overlays/index.ts — barrel for the leveling flow and the wizard chrome.
//
// Every Level stage (Set up · Level · Summary) is a look B page in `../level/`
// (`SetupPage`/`RunPage`/`SummaryPage`, each in `FlowPage`) — LevelingWizard is the
// switch that routes stage → page. `WizardShell`/`WizardFooter`/`WizTitle`/
// `StepRail` stay exported here for Doctor and Bulk Block Edit, which still use the
// centered-modal wizard chrome.
export { LevelingWizard } from "./LevelingWizard";
export type { LevelingWizardProps } from "./LevelingWizard";
export { WizardShell } from "./WizardShell";
export type { Stage, WizardShellProps } from "./WizardShell";
export { WizardFooter } from "./WizardFooter";
export type { WizardFooterProps } from "./WizardFooter";
export { WizTitle } from "./WizTitle";
export { StepRail } from "./StepRail";
export { WizardPage } from "./WizardPage";
export type { WizardPageProps } from "./WizardPage";
export { Pick } from "./Pick";
export type { PickOption, PickProps } from "./Pick";
