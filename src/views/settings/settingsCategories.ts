// src/views/settings/settingsCategories.ts — the Settings category rail: macOS System
// Settings-style, one category in the detail pane at a time. The rail selection is
// local UI state in SettingsView (not persisted, defaults to "targets").

import type { IconName } from "../../ui/Icon";

export type CategoryId = "targets" | "instruments" | "playback" | "about";

export interface Category {
  id: CategoryId;
  label: string;
  icon: IconName;
  desc: string;
}

export const CATEGORIES: Category[] = [
  {
    id: "targets",
    label: "Loudness targets",
    icon: "sliders",
    desc: "Your loudness targets. Set each level, rename it, and reorder it from its ⋯ menu.",
  },
  {
    id: "instruments",
    label: "Instruments",
    icon: "cable",
    desc: "Calibrate each instrument once. Then pick it for any preset when you level.",
  },
  {
    id: "playback",
    label: "Playback level",
    icon: "wave",
    desc: "The volume you play at. Bass targets adjust to match it.",
  },
  {
    id: "about",
    label: "About & updates",
    icon: "info",
    desc: "App version, updates, and where your data is kept.",
  },
];
