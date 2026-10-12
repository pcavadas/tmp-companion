// src/views/settings/SettingsView.tsx — the Settings tab (loudness targets +
// playback level + instrument profiles + Tier-2 calibration). The single Settings
// surface (the old standalone #settings WebviewWindow was removed). Wired to the
// real backend (no invented commands):
//   • getStore() → { profiles, targets, playback_level } on mount;
//     listPickupTopologies() for the Type/Pickup chips + per-row caption.
//   • saveTargets(next) / saveProfiles(next) / setPlaybackLevel(level) to persist.
//   • calibrateProfile(profileId, 8) for the real Tier-2 calibration (the backend
//     clamps 2..30 and returns the measured K-weighted LUFS). On resolve we re-read
//     getStore() so the persisted calibration_lufs refreshes the row.
//
// The App shell (App.tsx) renders the nav ABOVE this body, so this component
// renders only the category rail + detail pane (no AppNav/TabBar here). Layout
// per design_handoff_settings_rework (direction 1a, macOS System Settings
// style): a 210px rail of four categories — Loudness targets · Instruments ·
// Playback level · About & updates — and a right pane showing one category at
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
// Load and save are never silent. The store and the pickup topologies load
// together into a loading | error | ready state: nothing that edits a list is
// offered until it is ready, because save_targets / save_profiles replace the
// WHOLE list (and save_profiles unlinks the captures of every "removed" profile),
// so an add against the initial empty arrays would overwrite the stored list. A
// failed load shows an alert with Retry; a failed save shows an alert and
// re-reads the store, so the page shows what is actually stored.
//
// Split into ./TargetRow, ./PlaybackLevelSection, ./InstrumentRow,
// ./InstrumentForm; NeedsDevicePill stays co-located (shared by body + row).

import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import type { IconName } from "../../ui/Icon";
import { Rail, RailItem, RailLabel } from "../../ui/Rail";
import { ActionBar } from "../../ui/ActionBar";
import { AlertBanner, Button } from "../../ui/primitives";
import {
  getStore,
  listPickupTopologies,
  saveProfiles,
  saveTargets,
  setPlaybackLevel,
} from "../../lib/invoke";
import { errMsg } from "../../lib/format";
import type {
  PlaybackLevel,
  Profile,
  Store,
  Target,
  TopologyInfo,
} from "../../lib/types";
import type { UpdaterApi } from "../../lib/useUpdater";
import { TargetRow } from "./TargetRow";
import { InstrumentRow } from "./InstrumentRow";
import { InstrumentForm } from "./InstrumentForm";
import { PlaybackLevelSection } from "./PlaybackLevelSection";
import { AppUpdatesSection } from "./AppUpdatesSection";

// The wire target is name-only ({name, lufs}); the UI carries a transient `uid`
// so React keys, drag-reorder, and the auto-rename of a freshly added row stay
// stable across renames (names aren't unique) and persist (uid is stripped).
interface UiTarget extends Target {
  uid: string;
}
const stripUid = (rows: UiTarget[]): Target[] =>
  rows.map(({ name, lufs }) => ({ name, lufs }));

// Disabled "Needs device" pill — shown in place of the calibrate control when the
// unit is disconnected (calibration listens through the device input).
export function NeedsDevicePill() {
  const { t } = useTheme();
  return (
    <span
      title="Connect the Tone Master Pro to calibrate"
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: t.space3,
        height: 28,
        boxSizing: "border-box",
        fontFamily: t.sans,
        fontSize: t.fsUi,
        fontWeight: 500,
        color: t.faint,
        background: "transparent",
        border: `0.5px solid ${t.hairline}`,
        borderRadius: t.rMd,
        padding: `0 ${String(t.space5)}px`,
        whiteSpace: "nowrap",
        cursor: "not-allowed",
        opacity: 0.75,
      }}
    >
      <Icon name="cable" size={13} stroke={t.faint} />
      Needs device
    </span>
  );
}

interface EmptyHintProps {
  children: ReactNode;
}

// Italic muted empty-state bar shared by the Targets and Instruments panes.
function EmptyHint({ children }: EmptyHintProps) {
  const { t } = useTheme();
  return (
    <div
      style={{
        fontFamily: t.sans,
        fontSize: t.fsUi,
        color: t.faint,
        padding: `${String(t.space7)}px 0`,
        fontStyle: "italic",
      }}
    >
      {children}
    </div>
  );
}

// ===========================================================================
// Category rail — macOS System Settings-style sidebar. One category shows in
// the detail pane at a time; the rail selection is local UI state (not
// persisted, defaults to "targets").
// ===========================================================================

export type CategoryId = "targets" | "instruments" | "playback" | "about";

interface Category {
  id: CategoryId;
  label: string;
  icon: IconName;
  desc: string;
}

const CATEGORIES: Category[] = [
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

/** The initial read of the store + topologies. */
type LoadState =
  { kind: "loading" } | { kind: "error"; message: string } | { kind: "ready" };

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

  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [targets, setTargets] = useState<UiTarget[]>([]);
  const [playback, setPlayback] = useState<PlaybackLevel>("stage");
  const [topologies, setTopologies] = useState<TopologyInfo[]>([]);
  const [adding, setAdding] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  // Which category the detail pane shows — local UI state, not persisted.
  const [cat, setCat] = useState<CategoryId>(initialCategory ?? "targets");
  // uid of a freshly added target → opens it directly in rename mode.
  const [justAdded, setJustAdded] = useState<string | null>(null);
  const [load, setLoad] = useState<LoadState>({ kind: "loading" });
  // The last failed save, shown until the next save succeeds.
  const [saveError, setSaveError] = useState<string | null>(null);
  // Monotonic counter for transient target uids + drag source.
  const uidRef = useRef(0);
  const dragUid = useRef<string | null>(null);
  const newUid = () => `t${String(uidRef.current++)}`;
  // Bumped to (re-)read the store: by Retry after a failed load, and after a
  // failed save so the page shows what is actually stored.
  const [readGen, setReadGen] = useState(0);
  // Bumped as each save starts. A recovery read that sees it move discards its
  // result: a newer optimistic edit outranks what was stored before it.
  const saveSeqRef = useRef(0);

  const ready = load.kind === "ready";

  // Read the store and the topologies together; either failing is a load error.
  // A superseded read (a newer readGen, or unmount) applies nothing, and a read
  // overtaken by a newer save leaves that save's optimistic lists in place.
  useEffect(() => {
    let current = true;
    const seq = saveSeqRef.current;
    Promise.all([getStore(), listPickupTopologies()])
      .then(([store, tp]: [Store, TopologyInfo[]]) => {
        if (!current) return;
        if (saveSeqRef.current !== seq) {
          // A save started during this read; its own outcome decides the lists
          // (a failure re-reads again).
          setLoad({ kind: "ready" });
          return;
        }
        setProfiles(store.profiles);
        setTargets(
          // Stamp a transient uid per row (uidRef is stable across renders).
          store.targets.map((row) => ({
            ...row,
            uid: `t${String(uidRef.current++)}`,
          })),
        );
        setPlayback(store.playback_level);
        setTopologies(tp);
        setLoad({ kind: "ready" });
      })
      .catch((e: unknown) => {
        if (current) setLoad({ kind: "error", message: errMsg(e) });
      });
    return () => {
      current = false;
    };
  }, [readGen]);

  function retryLoad() {
    setLoad({ kind: "loading" });
    setReadGen((g) => g + 1);
  }

  // A save failed: say so, and re-read the store so the lists show what is
  // stored rather than the change that never landed.
  function saveFailed(what: string, e: unknown) {
    setSaveError(
      `Couldn't save your ${what}: ${errMsg(e)}. Showing what is saved instead.`,
    );
    setReadGen((g) => g + 1);
  }

  // Distinct instrument types (e.g. Guitar / Bass / Acoustic), in first-seen order.
  const types = useMemo(() => {
    const seen: string[] = [];
    for (const tp of topologies)
      if (!seen.includes(tp.instrument)) seen.push(tp.instrument);
    return seen;
  }, [topologies]);

  // topology_id → its TopologyInfo, for the per-row "Type · Pickup" caption.
  const topoById = useMemo(() => {
    const m = new Map<string, TopologyInfo>();
    for (const tp of topologies) m.set(tp.id, tp);
    return m;
  }, [topologies]);

  // Optimistic: the list updates at once; a failed save re-reads the store.
  async function persistProfiles(next: Profile[]) {
    setProfiles(next);
    saveSeqRef.current++;
    try {
      await saveProfiles(next);
      setSaveError(null);
    } catch (e) {
      saveFailed("instruments", e);
    }
  }

  // Persist a target list (strips the transient uid for the wire model).
  async function persistTargets(next: UiTarget[]) {
    setTargets(next);
    saveSeqRef.current++;
    try {
      await saveTargets(stripUid(next));
      setSaveError(null);
    } catch (e) {
      saveFailed("loudness targets", e);
    }
  }

  // ── Loudness targets — create / rename / set level / reorder / delete ──
  function addTarget() {
    const uid = newUid();
    // PR2 re-baseline: +3 from the mono-era -22.0 default (same audible loudness
    // under the new 2-ch BS.1770 convention — see TargetRow's TMIN/TMAX doc).
    void persistTargets([...targets, { uid, name: "New target", lufs: -19.0 }]);
    setJustAdded(uid);
  }
  function renameTarget(uid: string, name: string) {
    void persistTargets(
      targets.map((row) => (row.uid === uid ? { ...row, name } : row)),
    );
    if (justAdded === uid) setJustAdded(null);
  }
  function deleteTarget(uid: string) {
    void persistTargets(targets.filter((row) => row.uid !== uid));
  }
  function moveTarget(uid: string, dir: -1 | 1) {
    const i = targets.findIndex((r) => r.uid === uid);
    const j = i + dir;
    if (i < 0 || j < 0 || j >= targets.length) return;
    const next = [...targets];
    [next[i], next[j]] = [next[j], next[i]];
    void persistTargets(next);
  }
  function reorderTargets(fromUid: string | null, toUid: string) {
    if (!fromUid || fromUid === toUid) return;
    const next = [...targets];
    const fi = next.findIndex((r) => r.uid === fromUid);
    const ti = next.findIndex((r) => r.uid === toUid);
    if (fi < 0 || ti < 0) return;
    const [item] = next.splice(fi, 1);
    next.splice(ti, 0, item);
    void persistTargets(next);
  }

  const withLevel = (rows: UiTarget[], uid: string, lufs: number) =>
    rows.map((row) => (row.uid === uid ? { ...row, lufs } : row));
  // Optimistic per-move update (drives the fill/readout); does NOT persist.
  function setTargetLevelLocal(uid: string, lufs: number) {
    setTargets((prev) => withLevel(prev, uid, lufs));
  }
  // Persist ONCE on pointer-release (not on every drag move).
  function commitTargetLevel(uid: string, lufs: number) {
    void persistTargets(withLevel(targets, uid, lufs));
  }

  async function persistPlayback(level: PlaybackLevel) {
    setPlayback(level);
    saveSeqRef.current++;
    try {
      await setPlaybackLevel(level);
      setSaveError(null);
    } catch (e) {
      saveFailed("playback level", e);
    }
  }

  function addInstrument(data: { name: string; topology_id: string }) {
    const id = crypto.randomUUID();
    void persistProfiles([
      ...profiles,
      {
        id,
        name: data.name,
        topology_id: data.topology_id,
        calibration_lufs: null,
      },
    ]);
    setAdding(false);
  }

  function saveEdit(id: string, data: { name: string; topology_id: string }) {
    void persistProfiles(
      profiles.map((p) =>
        p.id === id
          ? { ...p, name: data.name, topology_id: data.topology_id }
          : p,
      ),
    );
    setEditingId(null);
  }

  function deleteProfile(id: string) {
    void persistProfiles(profiles.filter((p) => p.id !== id));
  }

  function moveProfile(id: string, dir: -1 | 1) {
    const i = profiles.findIndex((p) => p.id === id);
    const j = i + dir;
    if (i < 0 || j < 0 || j >= profiles.length) return;
    const next = [...profiles];
    [next[i], next[j]] = [next[j], next[i]];
    void persistProfiles(next);
  }

  // After a real calibration resolves, re-read the store so the persisted
  // calibration_lufs shows on the row. A failed re-read keeps the current rows:
  // the calibration itself already succeeded and was saved by the backend.
  async function refreshFromStore() {
    try {
      const store = await getStore();
      setProfiles(store.profiles);
    } catch {
      // nothing to refresh; the row keeps its current state.
    }
  }

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
      <style>{`@keyframes tmp-pulse{0%,100%{opacity:1}50%{opacity:.25}}.tmp-pulse{animation:tmp-pulse 1s ease-in-out infinite}`}</style>
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
                  small
                  icon="refresh"
                  onClick={retryLoad}
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
              <div style={{ maxWidth: 560 }}>
                {targets.map((row, i) => (
                  <TargetRow
                    key={row.uid}
                    name={row.name}
                    lufs={row.lufs}
                    defaultEditing={row.uid === justAdded}
                    onRename={(name) => {
                      renameTarget(row.uid, name);
                    }}
                    onChange={(v) => {
                      setTargetLevelLocal(row.uid, v);
                    }}
                    onCommit={(v) => {
                      commitTargetLevel(row.uid, v);
                    }}
                    onDelete={() => {
                      deleteTarget(row.uid);
                    }}
                    onMoveUp={
                      i > 0
                        ? () => {
                            moveTarget(row.uid, -1);
                          }
                        : undefined
                    }
                    onMoveDown={
                      i + 1 < targets.length
                        ? () => {
                            moveTarget(row.uid, 1);
                          }
                        : undefined
                    }
                    onGrab={() => {
                      dragUid.current = row.uid;
                    }}
                    onDropOn={() => {
                      reorderTargets(dragUid.current, row.uid);
                      dragUid.current = null;
                    }}
                    onDragEnd={() => {
                      dragUid.current = null;
                    }}
                  />
                ))}

                {targets.length === 0 && (
                  <EmptyHint>
                    No targets yet — add one to start leveling.
                  </EmptyHint>
                )}

                <Button
                  variant="ghost"
                  small
                  icon="plus"
                  onClick={addTarget}
                  style={{ marginTop: t.space6 }}
                >
                  Add target
                </Button>
              </div>
            )}

            {/* Kept MOUNTED (display-toggled) so a rail switch mid-calibration
                doesn't unmount InstrumentRow and discard an in-flight ~8 s
                device capture. */}
            <div
              style={{
                maxWidth: 620,
                display: cat === "instruments" && ready ? "block" : "none",
              }}
            >
              {!connected && (
                <div
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: t.space4,
                    padding: `${String(t.space4)}px ${String(t.space5)}px`,
                    marginBottom: t.space6,
                    borderRadius: t.rCard,
                    border: `0.5px solid ${t.hairlineStrong}`,
                    background: t.bgAlt,
                  }}
                >
                  <Icon
                    name="cable"
                    size={15}
                    stroke={t.sevWarn}
                    strokeWidth={1.5}
                  />
                  <span
                    style={{
                      fontFamily: t.sans,
                      fontSize: t.fsUi,
                      color: t.ink2,
                      lineHeight: 1.45,
                    }}
                  >
                    Your instruments and offsets are saved on this Mac.{" "}
                    <strong style={{ color: t.ink }}>
                      Calibrating needs the unit connected
                    </strong>
                    , because it listens through the device input.
                  </span>
                </div>
              )}

              <div
                style={{
                  fontFamily: t.sans,
                  fontSize: t.fsMeta,
                  color: t.mutedInk,
                  lineHeight: 1.45,
                  marginBottom: t.space6,
                }}
              >
                Play the way you gig: mix chords and lead, pick and fingers.
                Include EBow if you use one. Record it all in one take.
              </div>

              {profiles.map((p) =>
                editingId === p.id ? (
                  <InstrumentForm
                    key={p.id}
                    initial={p}
                    types={types}
                    topologies={topologies}
                    onSave={(d) => {
                      saveEdit(p.id, d);
                    }}
                    onCancel={() => {
                      setEditingId(null);
                    }}
                  />
                ) : (
                  <InstrumentRow
                    key={p.id}
                    profile={p}
                    topology={topoById.get(p.topology_id) ?? null}
                    connected={connected}
                    onCalibrated={() => {
                      void refreshFromStore();
                    }}
                    onEdit={() => {
                      setEditingId(p.id);
                    }}
                    onDelete={() => {
                      deleteProfile(p.id);
                    }}
                    onMove={(dir) => {
                      moveProfile(p.id, dir);
                    }}
                  />
                ),
              )}

              {profiles.length === 0 && (
                <EmptyHint>
                  No instruments yet — add one to calibrate.
                </EmptyHint>
              )}

              {adding ? (
                <InstrumentForm
                  types={types}
                  topologies={topologies}
                  onSave={addInstrument}
                  onCancel={() => {
                    setAdding(false);
                  }}
                />
              ) : (
                <Button
                  variant="ghost"
                  small
                  icon="plus"
                  onClick={() => {
                    setAdding(true);
                  }}
                  disabled={!ready || topologies.length === 0}
                  style={{ marginTop: t.space1 }}
                >
                  Add instrument
                </Button>
              )}
            </div>

            {cat === "playback" && ready && (
              <div style={{ maxWidth: 520 }}>
                <PlaybackLevelSection
                  value={playback}
                  onChange={(level) => {
                    void persistPlayback(level);
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

export default SettingsView;
