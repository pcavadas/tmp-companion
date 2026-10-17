// src/views/settings/SettingsView.tsx — the Settings tab (look B): a SplitView whose
// sidebar ("Settings categories") lists four pages — Loudness targets · Instruments ·
// Playback · About and updates — beside one content column (640 px at most) showing the
// chosen page, which scrolls on its own. The App shell draws the toolbar above it. No
// footer: the note on where the user's data is kept lives on About and updates.
//
// Wired to the real backend (no invented commands), through ./useSettingsStore:
//   • getStore() → { profiles, targets, playback_level, playback_channels } on mount,
//     with listPickupTopologies() for the instrument types, pickups and captions.
//   • saveTargets / saveProfiles / setPlaybackLevel / setPlaybackChannels to persist.
//   • calibrateProfile(profileId, 8) for an instrument recording (Tier-2 calibration:
//     the backend clamps 2..30 s and returns the measured loudness); on resolve the
//     store is re-read so the row shows what was saved.
//
// Load and save are never silent: a failed read shows a Notice with "Try again"; a
// failed save says what couldn't be saved and re-reads the store, so the page shows
// what is actually stored. Instruments stays mounted while another page shows, so a
// page switch mid-recording keeps the capture.
//
// Split into ./useSettingsStore (load + persistence), ./useTargets + ./TargetsPane,
// ./InstrumentsPane (+ ./useProfiles), ./PlaybackPane, ./AboutPane, ./StoreNotices,
// ./SettingsPage and ./settingsCategories.

import { useState } from "react";

import { RailItem } from "../../ui/Rail";
import { SplitView } from "../../ui/SplitView";
import type { UpdaterApi } from "../../lib/useUpdater";
import { AboutPane } from "./AboutPane";
import { InstrumentsPane } from "./InstrumentsPane";
import { PlaybackPane } from "./PlaybackPane";
import { TargetsPane } from "./TargetsPane";
import { CATEGORIES, type CategoryId } from "./settingsCategories";
import { useSettingsStore } from "./useSettingsStore";
import { useTargets } from "./useTargets";

export type { CategoryId };

export interface SettingsViewProps {
  connected: boolean;
  updater: UpdaterApi;
  /** Page to land on for this mount (e.g. the Level tab's "calibrate" cue jumping
   *  here) — read once at mount, not synced on later prop changes; omit for a plain
   *  tab entry, which defaults to "targets". */
  initialCategory?: CategoryId;
  /** Open the Report a bug dialog (About and updates' "Report a bug"; the native Help
   *  menu opens the same one). */
  onReportBug?: () => void;
}

export function SettingsView({
  connected,
  updater,
  initialCategory,
  onReportBug,
}: SettingsViewProps) {
  const store = useSettingsStore();
  const targetOps = useTargets(store);
  // Which page shows — local UI state, not persisted.
  const [cat, setCat] = useState<CategoryId>(initialCategory ?? "targets");

  return (
    <div
      style={{
        height: "100%",
        display: "flex",
        flexDirection: "column",
      }}
    >
      <SplitView
        sidebarLabel="Settings categories"
        sidebar={CATEGORIES.map((c) => (
          <RailItem
            key={c.id}
            label={c.label}
            active={c.id === cat}
            onClick={() => {
              setCat(c.id);
            }}
          />
        ))}
      >
        {cat === "targets" && <TargetsPane store={store} ops={targetOps} />}
        <InstrumentsPane
          visible={cat === "instruments"}
          connected={connected}
          store={store}
        />
        {cat === "playback" && <PlaybackPane store={store} />}
        {cat === "about" && (
          <AboutPane updater={updater} onReportBug={onReportBug} />
        )}
      </SplitView>
    </div>
  );
}
