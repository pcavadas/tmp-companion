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
// renders only the two-column grid + the spanning footer (no AppNav/TabBar here).
//
// Style rules honored: light-only; no grey-filled inputs (border-only/transparent
// inline-edit fields, terracotta focus border, no OS ring); 0.5px hairlines;
// shadows only on the floating ⋯ popover; Icons only (never symbol chars).
//
// Left column (handoff design_handoff_settings_targets): user-owned loudness
// targets — create / rename / reorder (drag) / delete + draggable slider, with
// NO upper ceiling/clamp — then the Playback level (Fletcher–Munson) section.
//
// Split into ./TargetRow, ./PlaybackLevelSection, ./InstrumentRow,
// ./InstrumentForm; NeedsDevicePill stays co-located (shared by body + row).

import { useEffect, useMemo, useRef, useState } from "react";

import { useTheme, useStyles } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { Button } from "../../ui/primitives";
import {
  getStore,
  listPickupTopologies,
  saveProfiles,
  saveTargets,
  setPlaybackLevel,
} from "../../lib/invoke";
import type {
  PlaybackLevel,
  Profile,
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
        gap: 6,
        height: 28,
        boxSizing: "border-box",
        fontFamily: t.sans,
        fontSize: t.fsUi,
        fontWeight: 500,
        color: t.faint,
        background: "transparent",
        border: `0.5px solid ${t.hairline}`,
        borderRadius: t.rMd,
        padding: "0 11px",
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

// ===========================================================================
// SettingsView — the page body (rendered under the App's nav).
// ===========================================================================

interface SettingsViewProps {
  connected: boolean;
  updater: UpdaterApi;
}

export function SettingsView({ connected, updater }: SettingsViewProps) {
  const { t } = useTheme();
  const s = useStyles();

  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [targets, setTargets] = useState<UiTarget[]>([]);
  const [playback, setPlayback] = useState<PlaybackLevel>("stage");
  const [topologies, setTopologies] = useState<TopologyInfo[]>([]);
  const [adding, setAdding] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  // uid of a freshly added target → opens it directly in rename mode.
  const [justAdded, setJustAdded] = useState<string | null>(null);
  // Monotonic counter for transient target uids + drag source.
  const uidRef = useRef(0);
  const dragUid = useRef<string | null>(null);
  const newUid = () => `t${String(uidRef.current++)}`;

  // Guard async post-await setState against unmount (tab switch).
  const mountedRef = useRef(true);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  useEffect(() => {
    getStore()
      .then((store) => {
        if (!mountedRef.current) return;
        setProfiles(store.profiles);
        setTargets(
          // Stamp a transient uid per row (uidRef is stable across renders).
          store.targets.map((row) => ({
            ...row,
            uid: `t${String(uidRef.current++)}`,
          })),
        );
        setPlayback(store.playback_level);
      })
      .catch(() => undefined);
    listPickupTopologies()
      .then((tp) => {
        if (mountedRef.current) setTopologies(tp);
      })
      .catch(() => undefined);
  }, []);

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

  async function persistProfiles(next: Profile[]) {
    setProfiles(next);
    try {
      await saveProfiles(next);
    } catch {
      // best-effort persist; UI already reflects the change.
    }
  }

  // Persist a target list (strips the transient uid for the wire model).
  async function persistTargets(next: UiTarget[]) {
    setTargets(next);
    try {
      await saveTargets(stripUid(next));
    } catch {
      // best-effort persist.
    }
  }

  // ── Loudness targets — create / rename / set level / reorder / delete ──
  function addTarget() {
    const uid = newUid();
    void persistTargets([...targets, { uid, name: "New target", lufs: -22.0 }]);
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
    try {
      await setPlaybackLevel(level);
    } catch {
      // best-effort persist; UI already reflects the change.
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
  // calibration_lufs shows on the row.
  async function refreshFromStore() {
    try {
      const store = await getStore();
      if (mountedRef.current) setProfiles(store.profiles);
    } catch {
      // leave the optimistic state; nothing to refresh.
    }
  }

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
      <div
        style={{
          flex: 1,
          minHeight: 0,
          overflowY: "auto",
          padding: "20px 26px",
          display: "grid",
          gridTemplateColumns: "1fr 1fr",
          gap: 30,
          alignContent: "start",
        }}
      >
        {/* ── LEFT — Loudness targets ── */}
        <div>
          <div style={s.kicker(t.accentDeep)}>Loudness targets · LUFS</div>
          <div
            style={{
              fontFamily: t.sans,
              fontSize: t.fsControl,
              color: t.mutedInk,
              margin: "6px 0 6px",
              lineHeight: 1.5,
            }}
          >
            The leveling target stack — drag a slider to set its level, rename
            to match how you play, or reorder to taste.
          </div>

          {targets.map((row) => (
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
              onGrab={() => {
                dragUid.current = row.uid;
              }}
              onDropOn={() => {
                reorderTargets(dragUid.current, row.uid);
              }}
            />
          ))}

          {targets.length === 0 && (
            <div
              style={{
                fontFamily: t.sans,
                fontSize: t.fsUi,
                color: t.faint,
                padding: "14px 0",
                fontStyle: "italic",
              }}
            >
              No targets yet — add one to start leveling.
            </div>
          )}

          <Button
            variant="ghost"
            small
            icon="plus"
            onClick={addTarget}
            style={{ marginTop: 12 }}
          >
            Add target
          </Button>

          {/* Playback level — Fletcher–Munson compensation for the targets above. */}
          <PlaybackLevelSection
            value={playback}
            onChange={(level) => {
              void persistPlayback(level);
            }}
          />
        </div>

        {/* ── RIGHT — Calibrated instruments ── */}
        <div style={{ minWidth: 0 }}>
          <div style={s.kicker(t.accentDeep)}>Calibrated instruments</div>
          <div
            style={{
              fontFamily: t.sans,
              fontSize: t.fsControl,
              color: t.mutedInk,
              margin: "6px 0 12px",
              lineHeight: 1.5,
            }}
          >
            Calibrate each guitar once — a short get-ready countdown, then ~8s
            of steady playing. Stored offsets let you{" "}
            <strong style={{ color: t.ink2 }}>
              level presets for any of them
            </strong>
            , chosen per preset at level time.
          </div>

          {!connected && (
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 9,
                padding: "9px 11px",
                marginBottom: 12,
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
                Your instruments and offsets are stored here, but{" "}
                <strong style={{ color: t.ink }}>
                  calibrating needs the unit connected
                </strong>{" "}
                — it listens through the device input.
              </span>
            </div>
          )}

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
              disabled={topologies.length === 0}
              style={{ marginTop: 2 }}
            >
              Add instrument
            </Button>
          )}

          <div style={{ marginTop: 24 }}>
            <AppUpdatesSection updater={updater} />
          </div>
        </div>

        {/* ── FOOTER — spans both columns ── */}
        <div
          style={{
            gridColumn: "1 / 3",
            marginTop: "auto",
            paddingTop: 14,
            borderTop: `0.5px solid ${t.hairline}`,
            display: "flex",
            alignItems: "center",
            gap: 10,
          }}
        >
          <Icon name="lock" size={13} stroke={t.mutedInk} />
          <span
            style={{
              fontFamily: t.mono,
              fontSize: t.fsMeta,
              color: t.mutedInk,
              letterSpacing: "0.03em",
            }}
          >
            The ONLY data this app stores locally. Everything else lives on the
            device.
          </span>
        </div>
      </div>
    </div>
  );
}

export default SettingsView;
