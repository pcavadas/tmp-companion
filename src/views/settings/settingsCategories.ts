// src/views/settings/settingsCategories.ts — the Settings sidebar's four pages, one shown
// at a time. The choice is local UI state in SettingsView (not saved; defaults to
// "targets").

export type CategoryId = "targets" | "instruments" | "playback" | "about";

export interface Category {
  id: CategoryId;
  label: string;
}

export const CATEGORIES: Category[] = [
  { id: "targets", label: "Loudness targets" },
  { id: "instruments", label: "Instruments" },
  { id: "playback", label: "Playback" },
  { id: "about", label: "About and updates" },
];
