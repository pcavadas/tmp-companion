// src/App.tsx — TMP Companion shell (click-only, 5-tab IA).
//
// A click-only 5-tab shell — Level / Copy / Songs / Catalog / Settings — routing to one
// self-gating <…View/> per tab under views/:
//   • Level    → <LevelView/>   now-playing strip + preset list + the §4
//                destructive-write ritual, bound to real invoke wrappers.
//   • Copy     → <CopyView/>     copy blocks between presets (staged off-device).
//   • Songs    → <SongsView/>    device-backed songs + setlists CRUD; renders its
//                own in-body EmptyState until the unit is connected.
//   • Catalog  → <CatalogView/>  device-independent reference catalog.
//   • Settings → <SettingsView/> instrument profiles + Tier-2 calibration.
//
// Connection is fully automatic: connect_device on mount, retry every 3 s on a
// guarded interval (inert while connected, live again after an unplug), a
// friendly "please connect your Tone Master Pro" gate when no device, and
// actionable failures (e.g. "close Pro Control") surfaced as a red alert. The
// backend hotplug watcher emits tmp://device-{attached,detached} so unplugging
// drops the UI to disconnected and replugging reconnects immediately. Click-only:
// no keyboard shortcuts, no command palette. The tab bar is a minimal click-only
// bar reading the shared theme tokens.

import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { useTheme } from "./theme/ThemeContext";
import { AlertBanner } from "./ui/primitives";
import { ErrorBoundary } from "./ui/ErrorBoundary";
import { DeviceStatus } from "./ui/DeviceStatus";
import { Disclaimer } from "./views/Disclaimer";
import { BugReportDialog } from "./views/BugReportDialog";
import { LevelView } from "./views/level";
import { DoctorView } from "./views/doctor";
import { CopyView } from "./views/copy";
import { SongsView } from "./views/songs";
import { CatalogView } from "./views/CatalogView";
import { SettingsView } from "./views/settings";
import type { CategoryId } from "./views/settings";
import { FirmwareGate } from "./views/FirmwareGate";
import { firmwareGateActive, firmwareSupported } from "./lib/firmware";
import { connectDevice, currentGraph, isTauri } from "./lib/invoke";
import { useUpdater } from "./lib/useUpdater";
import { UpdateOverlay } from "./ui/UpdateOverlay";
import { ensureLibraryScan, resetLibraryScan } from "./views/level/libraryScan";
import { startScanAfterGraph } from "./lib/scheduleLibraryScan";
import type { ActiveGraph } from "./lib/types";
import { actionableError } from "./lib/connectError";
import { DISCLAIMER_PERM_KEY, DISCLAIMER_SESSION_KEY } from "./lib/gates";

const RETRY_MS = 3000; // legacy auto-connect retry cadence.

// Route keys, labels, and view components all agree (no Presets/Models discrepancy):
// Level · Doctor · Copy · Songs · Catalog · Settings — the leveling page is `level` →
// <LevelView>, the reference catalog is `catalog` → <CatalogView>.
type Tab = "level" | "doctor" | "copy" | "songs" | "catalog" | "settings";
type ConnStatus = "connecting" | "connected" | "disconnected";

// `deviceIndependent` tabs render without a connected TMP, so the connection
// gate is suppressed while they're active (Catalog is a static reference catalog).
// Doctor is device-dependent (no `deviceIndependent`) and stays firmware-gated.
const TABS: { id: Tab; label: string; deviceIndependent?: boolean }[] = [
  { id: "level", label: "Level" },
  { id: "doctor", label: "Doctor" },
  { id: "copy", label: "Copy" },
  { id: "songs", label: "Songs" },
  { id: "catalog", label: "Catalog", deviceIndependent: true },
  { id: "settings", label: "Settings" },
];

export default function App() {
  // The single ThemeProvider lives in main.tsx (wraps this root); App is the shell.
  return <AppShell />;
}

function AppShell() {
  const { t } = useTheme();
  const updater = useUpdater();

  const [disclaimerOk, setDisclaimerOk] = useState(
    () =>
      localStorage.getItem(DISCLAIMER_PERM_KEY) === "1" ||
      sessionStorage.getItem(DISCLAIMER_SESSION_KEY) === "1",
  );

  const [tab, setTab] = useState<Tab>("level");
  // Settings category to seed on the NEXT Settings mount — set by the Level tab's
  // "calibrate" cue (jump to Instruments); null = plain entry, defaults to
  // "targets". A manual tab click always clears it (below) so a later ordinary
  // Settings visit doesn't inherit a stale Instruments jump.
  const [settingsCategory, setSettingsCategory] = useState<
    CategoryId | undefined
  >(undefined);
  const selectTab = useCallback((next: Tab) => {
    setSettingsCategory(undefined);
    setTab(next);
  }, []);
  const onCalibrate = useCallback(() => {
    setSettingsCategory("instruments");
    setTab("settings");
  }, []);
  const [status, setStatus] = useState<ConnStatus>("connecting");
  // Firmware the connected unit reported during the handshake (connect_device
  // resolves with it). Never cached across a disconnect — no unit, no version.
  const [firmware, setFirmware] = useState<string | null>(null);
  const [connectError, setConnectError] = useState<string | null>(null);
  // The active graph from the combined handshake — delivered alongside the
  // firmware in a single burst so the signal chain renders immediately.
  const [initialGraph, setInitialGraph] = useState<ActiveGraph | null>(null);

  const connectedRef = useRef(false);
  const connectingRef = useRef(false);

  // ── Fully-automatic connection (combined handshake) ───────────────────────
  const connect = useCallback(async () => {
    try {
      const result = await connectDevice();
      connectedRef.current = true;
      setFirmware(result.firmware ?? null);
      setInitialGraph(result.graph ?? null);
      setStatus("connected");
      setConnectError(null);
      // ONE library backup scan per connection, owned here so every device tab
      // (Level/Copy/Songs) consumes the SAME scan — switching tabs never re-triggers
      // it. Idempotent (module-scoped guard): a reconnect after detach scans fresh.
      // HOLD it until the active signal path is in hand: the ~22 s backup pauses the
      // monitor, so kicking it on a graph=none snapshot preempts the monitor's
      // graph-retry and the hero stays blank for the whole backup. Poll the cheap
      // cached graph first (bounded), forwarding it so the hero paints right away.
      void startScanAfterGraph({
        initialGraph: result.graph ?? null,
        getGraph: currentGraph,
        onGraph: setInitialGraph,
        startScan: ensureLibraryScan,
      });
    } catch (e) {
      const msg =
        e instanceof Error
          ? e.message
          : typeof e === "object" && e !== null && "message" in e
            ? String(e.message)
            : String(e);
      // A booting/just-replugged unit fails the first handshake send with a
      // timeout — suppressed by actionableError into the friendly gate, not a
      // red banner; the retry loop reconnects once the unit is up.
      setConnectError(actionableError(msg));
      if (!connectedRef.current) setStatus("disconnected");
    }
  }, []);

  // Guarded single-flight attempt — shared by the 3 s poll and the hotplug
  // attached event. A no-op while connected or while another attempt runs.
  const attempt = useCallback(async () => {
    if (connectingRef.current || connectedRef.current) return;
    connectingRef.current = true;
    try {
      await connect();
    } finally {
      connectingRef.current = false;
    }
  }, [connect]);

  useEffect(() => {
    void attempt();
    // The interval stays armed for the app's life: ticks are no-op guard checks
    // while connected, and become live retries again after an unplug (the
    // detach event flips connectedRef back to false).
    const id = setInterval(() => void attempt(), RETRY_MS);
    return () => {
      clearInterval(id);
    };
  }, [attempt]);

  // ── Hotplug events from the backend watcher (non-seizing IOHIDManager) ────
  useEffect(() => {
    if (!isTauri()) return; // inert under Vitest/jsdom — no Tauri event bridge
    const unlistens: Promise<() => void>[] = [
      listen("tmp://device-detached", () => {
        connectedRef.current = false;
        setStatus("disconnected");
        setFirmware(null);
        setInitialGraph(null);
        setConnectError(null);
        resetLibraryScan(); // next connection scans fresh
      }),
      listen("tmp://device-attached", () => {
        // Fresh attach — try connecting immediately instead of waiting for the
        // next 3 s poll tick (the unit may still be booting; a SetReport-timeout
        // failure is suppressed into the gate, and the poll keeps retrying).
        void attempt();
      }),
    ];
    return () => {
      for (const u of unlistens)
        void u.then((f) => {
          f();
        });
    };
  }, [attempt]);

  // ── Help → "Report a Bug…" (native menu item, bootstrap.rs) ───────────────
  const [bugReportOpen, setBugReportOpen] = useState(false);
  useEffect(() => {
    if (!isTauri()) return;
    const unlisten = listen("tmp://open-bug-report", () => {
      setBugReportOpen(true);
    });
    return () => {
      void unlisten.then((f) => {
        f();
      });
    };
  }, []);

  // Manual retry (from the gate button) — forces another attempt immediately.
  const retry = useCallback(() => {
    setStatus("connecting");
    setConnectError(null);
    void connect();
  }, [connect]);

  const connected = status === "connected";

  // ── Untested-firmware gate ────────────────────────────────────────────────
  // Below-floor firmware shows a full-page notice on the unit-driven tabs until
  // the user proceeds. The override is per-mount and RESETS when the unit clears
  // the floor again (or unplugs → firmware null), so a later re-downgrade
  // re-shows the notice. Reset via the render-phase prev-compare pattern (a
  // synchronous setState in an effect is forbidden by the strict eslint config).
  const fwOk = firmwareSupported(firmware);
  const [proceeded, setProceeded] = useState(false);
  const [prevFwOk, setPrevFwOk] = useState(fwOk);
  if (prevFwOk !== fwOk) {
    setPrevFwOk(fwOk);
    if (fwOk) setProceeded(false);
  }

  const acceptDisclaimer = (permanent: boolean) => {
    sessionStorage.setItem(DISCLAIMER_SESSION_KEY, "1");
    if (permanent) localStorage.setItem(DISCLAIMER_PERM_KEY, "1");
    setDisclaimerOk(true);
  };

  // ── Device-status indicator — dot carries the link state, label carries the
  //    firmware the unit reported (design handoff: firmware_status). ──────────
  const statusNode = (
    <DeviceStatus connected={connected} firmwareVersion={firmware} />
  );

  // The bug-report dialog is a Help-menu-native affordance — it must mount
  // regardless of the disclaimer gate, else a click on the disclaimer screen
  // sets bugReportOpen with nothing rendered and the dialog surprise-opens
  // the instant the disclaimer is accepted.
  const bugReportDialog = bugReportOpen && (
    <BugReportDialog
      connected={connected}
      firmware={firmware}
      onClose={() => {
        setBugReportOpen(false);
      }}
    />
  );

  if (!disclaimerOk) {
    return (
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          height: "100vh",
          background: t.bg,
          color: t.ink,
          fontFamily: t.sans,
          overflow: "hidden",
        }}
      >
        <Disclaimer onAccept={acceptDisclaimer} />
        {bugReportDialog}
      </div>
    );
  }

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        height: "100vh",
        background: t.bg,
        color: t.ink,
        fontFamily: t.sans,
        overflow: "hidden",
        position: "relative",
      }}
    >
      <TabBar tab={tab} onSelect={selectTab} statusNode={statusNode} />

      {/* Actionable connect error (e.g. "close Pro Control") — red alert. */}
      {connectError && (
        <AlertBanner
          style={{
            margin: `${String(t.space5)}px ${String(t.space8)}px 0`,
            padding: `${String(t.space4)}px ${String(t.space6)}px`,
            fontSize: t.fsUi,
            flexShrink: 0,
          }}
        >
          {connectError}
        </AlertBanner>
      )}

      {/* Active workspace. A per-tab ErrorBoundary keyed on `tab` isolates a
          body crash to the workspace — the tab bar + connection state survive,
          and switching tabs remounts a fresh boundary (so a crashed tab doesn't
          leave its fallback showing on the next one). */}
      <div
        style={{
          flex: 1,
          minHeight: 0,
          display: "flex",
          flexDirection: "column",
          // Positioning context for a tab's full-page `absolute; inset:0` body
          // swaps (Doctor Setup/Results) so they fill the workspace and never
          // cover the tab bar — Level/Copy already scope their own; this backstops
          // any tab (like Doctor) that doesn't. // ponytail: shell-level guard.
          position: "relative",
        }}
      >
        <ErrorBoundary key={tab}>
          {firmwareGateActive({ firmware, tab, proceeded }) &&
          firmware != null ? (
            <FirmwareGate
              detected={firmware}
              onCheckAgain={retry}
              onProceed={() => {
                setProceeded(true);
              }}
            />
          ) : (
            <>
              {tab === "level" && (
                <LevelView
                  connected={connected}
                  onScan={retry}
                  initialGraph={initialGraph}
                  onCalibrate={onCalibrate}
                />
              )}
              {tab === "doctor" && (
                <DoctorView connected={connected} onScan={retry} />
              )}
              {tab === "copy" && (
                <CopyView
                  connected={connected}
                  onScan={retry}
                  initialGraph={initialGraph}
                />
              )}
              {tab === "songs" && (
                <SongsView connected={connected} onScan={retry} />
              )}
              {tab === "catalog" && <CatalogView />}
              {tab === "settings" && (
                <SettingsView
                  connected={connected}
                  updater={updater}
                  initialCategory={settingsCategory}
                />
              )}
            </>
          )}
        </ErrorBoundary>
      </div>

      {/* Auto-update surface — the phase-driven toast/modal, above everything. */}
      <UpdateOverlay u={updater} />

      {bugReportDialog}
    </div>
  );
}

// ── Top tab bar (click-only, 4-tab IA) ────────────────────────────────────────
interface TabBarProps {
  tab: Tab;
  onSelect: (t: Tab) => void;
  statusNode: React.ReactNode;
}

function TabBar({ tab, onSelect, statusNode }: TabBarProps) {
  const { t } = useTheme();
  return (
    <div
      style={{
        height: 46,
        flexShrink: 0,
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        padding: `0 ${String(t.space8)}px`,
        borderBottom: `0.5px solid ${t.hairline}`,
        background: t.titlebar,
      }}
    >
      <div style={{ display: "flex", gap: t.space1 }}>
        {TABS.map((tb) => {
          const on = tb.id === tab;
          return (
            <button
              key={tb.id}
              onClick={() => {
                onSelect(tb.id);
              }}
              style={{
                border: "none",
                cursor: "pointer",
                fontFamily: t.sans,
                fontSize: t.fsBody,
                fontWeight: on ? 600 : 450,
                color: on ? t.ink : t.mutedInk,
                padding: `${String(t.space3)}px ${String(t.space6)}px`,
                borderRadius: t.rBtn,
                background: on ? t.accentSoft : "transparent",
              }}
            >
              {tb.label}
            </button>
          );
        })}
      </div>
      {statusNode}
    </div>
  );
}
