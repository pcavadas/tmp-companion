// src/ui/iconNames.ts — the runtime icon-name registry + the `IconName` union
// derived from it (kept together so the array and the type can never drift apart).
// Split from ./Icon so that component file exports only the Icon component
// (React Fast Refresh's component-boundary rule) without disabling it.

// Runtime list of every icon name (handoff bundle export).
export const ICONS = [
  "search",
  "plus",
  "settings",
  "share",
  "save",
  "star",
  "folder",
  "wave",
  "mic",
  "sliders",
  "list",
  "grid",
  "cmd",
  "arrow-right",
  "arrow-down",
  "check",
  "chev-right",
  "chev-down",
  "x",
  "more",
  "metro",
  "tune",
  "rules",
  "footswitch",
  "gauge",
  "cable",
  "music",
  "spinner",
  "warn-tri",
  "refresh",
  "undo",
  "redo",
  "grip",
  "trash",
  "lock",
  "shield",
  "play",
  "pause",
] as const;

export type IconName = (typeof ICONS)[number];
