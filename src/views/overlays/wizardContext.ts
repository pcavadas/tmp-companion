// src/views/overlays/wizardContext.ts — the leveling flow's stage union, shared by the
// stage switch (`LevelingWizard`) and the store that owns the flow (`levelRunStore`).
// Split from the switch so that file exports only components (React Fast Refresh's
// component-boundary rule).

export type Stage = "closed" | "setup" | "run" | "summary";
