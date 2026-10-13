// src/views/settings/SettingsView.tsx — the Settings tab (loudness targets +
// playback level and outputs + instrument profiles + Tier-2 calibration). The single Settings
// surface (the old standalone #settings WebviewWindow was removed). Wired to the
// real backend (no invented commands):
//   • getStore() → { profiles, targets, playback_level, playback_channels } on mount;
//     listPickupTopologies() for the Type/Pickup chips + per-row caption.
//   • saveTargets(next) / saveProfiles(next) / setPlaybackLevel(level) /
//     setPlaybackChannels(channels) to persist.
//   • calibrateProfile(profileId, 8) for the real Tier-2 calibration (the backend
//     clamps 2..30 and returns the measured K-weighted LUFS). On resolve we re-read
//     getStore() so the persisted calibration_lufs refreshes the row.
//
// The App shell (App.tsx) renders the nav ABOVE this body, so this component
// renders only the category rail + detail pane (no AppNav/TabBar here). Layout
// per design_handoff_settings_rework (direction 1a, macOS System Settings
// style): a 210px rail of four categories — Loudness targets · Instruments ·
// Playback · About & updates — and a right pane showing one category at
// a time that scrolls on its own, so a growing list can never push the other
// sections off-screen.
//
// Style rules honored: light-only; no grey-filled inputs (border-only/transparent
// inline-edit fields, terracotta focus border, no OS ring); 0.5px hairlines;
// shadows only on the floating ⋯ popover; Icons only (never symbol chars).
//
// Targets (handoff design_handoff_settings_targets): user-owned loudness
// targets — create / rename / reorder (⋯ Move up/down, or drag) / delete +
// draggable slider, with NO upper ceiling/clamp.
//
// Load and save are never silent (see ./useSettingsStore): a failed load shows an
// alert with Retry; a failed save shows an alert and re-reads the store, so the page
// shows what is actually stored.
//
// Split into ./useSettingsStore (load + persistence), ./useTargets, ./TargetsPane,
// ./InstrumentsPane (+ ./useProfiles), ./PlaybackLevelSection,
// ./PlaybackChannelsSection, ./AppUpdatesSection and ./settingsCategories.

import { useState } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { Rail, RailItem, RailLabel } from "../../ui/Rail";
import { ActionBar } from "../../ui/ActionBar";
import { AlertBanner, Button } from "../../ui/primitives";
import type { UpdaterApi } from "../../lib/useUpdater";
import { PlaybackLevelSection } from "./PlaybackLevelSection";
import { PlaybackChannelsSection } from "./PlaybackChannelsSection";
import { AppUpdatesSection } from "./AppUpdatesSection";
import { EmptyHint } from "./EmptyHint";
import { InstrumentsPane } from "./InstrumentsPane";
import { TargetsPane } from "./TargetsPane";
import { CATEGORIES, type CategoryId } from "./settingsCategories";
import { useSettingsStore } from "./useSettingsStore";
import { useTargets } from "./useTargets";

export type { CategoryId };

// ===========================================================================
// SettingsView — the page body (rendered under the App's nav).
// ===========================================================================

export interface SettingsViewProps {
  connected: boolean;
  updater: UpdaterApi;
  /** Category to land on for this mount (e.g. the Level tab's "calibrate" cue
   *  jumping here) — read once at mount, not synced on later prop changes;
   *  omit for a plain tab entry, which defaults to "targets". */
  initialCategory?: CategoryId;
}

export function SettingsView({
  connected,
  updater,
  initialCategory,
}: SettingsViewProps) {
  const { t } = useTheme();

  const store = useSettingsStore();
  const { load, ready, saveError } = store;
  const targetOps = useTargets(store);
  // Which category the detail pane shows — local UI state, not persisted.
  const [cat, setCat] = useState<CategoryId>(initialCategory ?? "targets");

  const active = CATEGORIES.find((c) => c.id === cat) ?? CATEGORIES[0];

  return (
    <div
      style={{
        height: "100%",
        display: "flex",
        flexDirection: "column",
        background: t.bg,
        overflow: "hidden",
      }}
    >
      <div style={{ flex: 1, minHeight: 0, display: "flex" }}>
        {/* ── RAIL — category sidebar (the DS Rail, shared with Songs) ── */}
        <Rail>
          <RailLabel
            style={{
              padding: `${String(t.space2)}px ${String(t.space5)}px ${String(t.space4)}px`,
            }}
          >
            Settings
          </RailLabel>
          <div
            role="tablist"
            aria-label="Settings categories"
            style={{ display: "flex", flexDirection: "column", gap: t.space1 }}
          >
            {CATEGORIES.map((c) => (
              <RailItem
                nav="tab"
                key={c.id}
                label={c.label}
                icon={c.icon}
                active={c.id === cat}
                onClick={() => {
                  setCat(c.id);
                }}
              />
            ))}
          </div>
        </Rail>

        {/* ── PANE — one category at a time, scrolls on its own ── */}
        <div
          style={{
            flex: 1,
            minWidth: 0,
            display: "flex",
            flexDirection: "column",
          }}
        >
          <div
            style={{
              flexShrink: 0,
              padding: `${String(t.space8)}px ${String(t.space10)}px ${String(t.space7)}px`,
              borderBottom: `0.5px solid ${t.hairline}`,
            }}
          >
            <div
              style={{
                fontFamily: t.serif,
                fontSize: t.fsSheet,
                color: t.ink,
                letterSpacing: "-0.01em",
              }}
            >
              {active.label}
            </div>
            <div
              style={{
                fontFamily: t.sans,
                fontSize: t.fsControl,
                color: t.mutedInk,
                lineHeight: 1.5,
                marginTop: t.space3,
                maxWidth: 520,
                textWrap: "pretty",
              }}
            >
              {active.desc}
            </div>
          </div>

          <div
            className="tmp-pane-scroll"
            style={{
              flex: 1,
              minHeight: 0,
              overflowY: "auto",
              padding: `${String(t.space8)}px ${String(t.space10)}px ${String(t.space10)}px`,
            }}
          >
            {load.kind === "error" && cat !== "about" && (
              <AlertBanner
                style={{
                  marginBottom: t.space8,
                  maxWidth: 560,
                  display: "flex",
                  alignItems: "center",
                  gap: t.space6,
                }}
              >
                <span style={{ flex: 1 }}>
                  Couldn&rsquo;t read your settings: {load.message}
                </span>
                <Button
                  variant="ghost"
                  size="sm"
                  icon="refresh"
                  onClick={store.retryLoad}
                >
                  Retry
                </Button>
              </AlertBanner>
            )}
            {saveError != null && cat !== "about" && (
              <AlertBanner style={{ marginBottom: t.space8, maxWidth: 560 }}>
                {saveError}
              </AlertBanner>
            )}
            {load.kind === "loading" && cat !== "about" && (
              <EmptyHint>Reading your settings…</EmptyHint>
            )}
            {cat === "targets" && ready && (
              <TargetsPane targets={store.targets} ops={targetOps} />
            )}

            <InstrumentsPane
              visible={cat === "instruments" && ready}
              ready={ready}
              connected={connected}
              profiles={store.profiles}
              topologies={store.topologies}
              persistProfiles={store.persistProfiles}
              onRefresh={store.refreshFromStore}
            />

            {cat === "playback" && ready && (
              <div style={{ maxWidth: 520 }}>
                <PlaybackLevelSection
                  value={store.playback}
                  onChange={(level) => {
                    void store.persistPlayback(level);
                  }}
                />
                <div
                  style={{
                    marginTop: t.space8,
                    fontFamily: t.sans,
                    fontSize: t.fsLabel,
                    color: t.faint,
                    lineHeight: 1.5,
                    fontStyle: "italic",
                    maxWidth: 480,
                    textWrap: "pretty",
                  }}
                >
                  Your ears hear less bass at low volume. So at quiet and
                  rehearsal levels, bass presets get a small boost to sound as
                  full as they do on stage.
                </div>
                <div style={{ marginTop: t.space12 }}>
                  <PlaybackChannelsSection
                    value={store.channels}
                    onChange={(next) => {
                      void store.persistChannels(next);
                    }}
                  />
                </div>
              </div>
            )}

            {cat === "about" && (
              <div style={{ maxWidth: 560 }}>
                <AppUpdatesSection updater={updater} />
              </div>
            )}
          </div>
        </div>
      </div>

      {/* ── shared bottom bar — full window width, every category (DS ActionBar) ── */}
      <ActionBar
        left={
          <span
            style={{
              fontFamily: t.mono,
              fontSize: t.fsMeta,
              color: t.faint,
              display: "inline-flex",
              gap: t.space4,
              alignItems: "center",
            }}
          >
            <Icon name="lock" size={13} stroke={t.faint} />
            Instruments and loudness targets are the only data this app keeps on
            your Mac. Presets, scenes, and songs live on the device, not here.
          </span>
        }
        right={null}
      />
    </div>
  );
}
