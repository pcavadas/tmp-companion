// src/App.tsx — TMP Companion shell (click-only, 6-tab IA).
//
// A click-only 6-tab shell — Level / Doctor / Copy / Songs / Catalog / Settings — routing to one
// self-gating <…View/> per tab under views/:
//   • Level    → <LevelView/>   now-playing strip + preset list + the §4
//                destructive-write ritual, bound to real invoke wrappers.
//   • Doctor   → <DoctorView/>   tone diagnosis: re-amp captures, spectral
//                findings, fixes described as steps on the unit (it never writes).
//   • Copy     → <CopyView/>     copy blocks between presets (staged off-device).
//   • Songs    → <SongsView/>    device-backed songs + setlists CRUD.
// The four device tabs render the shared <DeviceState/> page until a unit connects.
//   • Catalog  → <CatalogView/>  device-independent reference catalog.
//   • Settings → <SettingsView/> instrument profiles + Tier-2 calibration.
//
// Connection is fully automatic: connect_device on mount, retry every 3 s on a
// guarded interval (inert while connected, live again after an unplug), and the
// backend hotplug watcher's tmp://device-{attached,detached} events — all through ONE
// single-flight attempt. There is no Connect, Disconnect or Scan button anywhere. While
// no unit is connected, the device tabs show one DeviceState page picked by the last
// failed connect's typed KIND (plug it in / another app is using it / couldn't connect);
// the backend message only reaches the log. The toolbar's DeviceStatus says the same in
// a few words. Click-only: no keyboard shortcuts, no command palette.

import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { useTheme } from "./theme/ThemeContext";
import { ErrorBoundary } from "./ui/ErrorBoundary";
import { DeviceStatus, type DeviceStatusState } from "./ui/DeviceStatus";
import { Toolbar } from "./ui/Toolbar";
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
import { LibraryScanNotice } from "./views/LibraryScanNotice";
import { firmwareGateActive, firmwareSupported } from "./lib/firmware";
import { connectDevice, currentGraph, isTauri } from "./lib/invoke";
import { useUpdater } from "./lib/useUpdater";
import { UpdateOverlay } from "./ui/UpdateOverlay";
import { ensureLibraryScan, resetLibraryScan } from "./device/libraryScan";
import { onDeviceDetached as resetLevelOnDetach } from "./views/level/levelRunStore";
import { startScanAfterGraph } from "./lib/scheduleLibraryScan";
import type { ActiveGraph, CommandErrorKind } from "./lib/types";
import { errorKind, noUnitState } from "./lib/connectError";
import { loadPlatform } from "./lib/platform";
import { DISCLAIMER_PERM_KEY, DISCLAIMER_SESSION_KEY } from "./lib/gates";

const RETRY_MS = 3000; // auto-connect retry interval (armed while disconnected).

// Route keys, labels, and view components all agree (no Presets/Models discrepancy):
// Level · Doctor · Copy · Songs · Catalog · Settings — the leveling page is `level` →
// <LevelView>, the reference catalog is `catalog` → <CatalogView>.
type Tab = "level" | "doctor" | "copy" | "songs" | "catalog" | "settings";
type ConnStatus = "connecting" | "connected" | "disconnected";

// Which tabs the firmware gate covers is decided by FW_UNGATED_TABS in
// lib/firmware.ts, not here.
const TABS: readonly { id: Tab; label: string }[] = [
  { id: "level", label: "Level" },
  { id: "doctor", label: "Doctor" },
  { id: "copy", label: "Copy" },
  { id: "songs", label: "Songs" },
  { id: "catalog", label: "Catalog" },
  { id: "settings", label: "Settings" },
];

// The single ThemeProvider lives in main.tsx (wraps this root); App is the shell.
export default function App() {
  const { t } = useTheme();
  const updater = useUpdater();

  const [disclaimerOk, setDisclaimerOk] = useState(
    () =>
      localStorage.getItem(DISCLAIMER_PERM_KEY) === "1" ||
      sessionStorage.getItem(DISCLAIMER_SESSION_KEY) === "1",
  );

  const [tab, setTab] = useState<Tab>("level");
  // Settings category to seed on the NEXT Settings mount — set by the links into it
  // ("Record yours in Settings" in Level and Doctor jumps to Instruments, Set up's "Add a
  // target in Settings" to Loudness targets); null = plain entry, defaults to "targets".
  // A manual tab click always clears it (below) so a later ordinary Settings visit
  // doesn't inherit a stale jump.
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
  // Set up's "Add a target in Settings": jump to Loudness targets.
  const onAddTarget = useCallback(() => {
    setSettingsCategory("targets");
    setTab("settings");
  }, []);
  const [status, setStatus] = useState<ConnStatus>("connecting");
  // Firmware the connected unit reported during the handshake (connect_device
  // resolves with it). Never cached across a disconnect — no unit, no version.
  const [firmware, setFirmware] = useState<string | null>(null);
  // The typed KIND of the last failed connect (null once one succeeds, and after a
  // detach). It picks the device tabs' page and the toolbar's words; the message text
  // never reaches the screen (invoke() already logged it).
  const [connectKind, setConnectKind] = useState<CommandErrorKind | null>(null);
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
      setConnectKind(null);
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
      // The error's KIND picks the page (no unit or a booting one, another app
      // holding it, anything else); the retry loop reconnects once that clears.
      setConnectKind(errorKind(e));
      if (!connectedRef.current) setStatus("disconnected");
    }
  }, []);

  // Guarded single-flight attempt — the ONE connect path, shared by the 3 s poll and
  // the hotplug attached event. A no-op while connected or while another attempt runs.
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
        setConnectKind(null);
        resetLibraryScan(); // next connection scans fresh
        resetLevelOnDetach(); // a reconnect can re-map the list indexes a run keyed on
      }),
      listen("tmp://device-attached", () => {
        // Fresh attach — try connecting immediately instead of waiting for the
        // next 3 s poll tick (the unit may still be booting; a SetReport-timeout
        // failure shows the plug-it-in page, and the poll keeps retrying).
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

  // Which OS this build runs on (the busy page's wording) — read once, device-free.
  useEffect(() => {
    void loadPlatform();
  }, []);

  // ── Report a bug: the native Help menu item (bootstrap.rs) and Settings › About ──
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

  // What the device tabs show while no unit is connected, and the toolbar's words.
  const noUnit = noUnitState(status === "connecting", connectKind);
  const statusState: DeviceStatusState = connected
    ? fwOk
      ? "connected"
      : "untested"
    : noUnit === "connecting"
      ? "connecting"
      : noUnit === "busy"
        ? "busy"
        : "off";
  const openBugReport = useCallback(() => {
    setBugReportOpen(true);
  }, []);

  // The bug-report dialog must mount regardless of the disclaimer gate (the Help menu
  // reaches it there), else a click on the disclaimer screen sets bugReportOpen with
  // nothing rendered and the dialog surprise-opens the instant the disclaimer is accepted.
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
          background: t.window,
          color: t.text,
          fontFamily: t.fontSans,
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
        background: t.window,
        color: t.text,
        fontFamily: t.fontSans,
        overflow: "hidden",
        position: "relative",
      }}
    >
      <Toolbar
        tabs={TABS}
        current={tab}
        onSelect={selectTab}
        status={<DeviceStatus state={statusState} firmwareVersion={firmware} />}
      />

      {/* A failed library scan — said once, app-wide, under the toolbar. */}
      {connected && <LibraryScanNotice />}

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
              onProceed={() => {
                setProceeded(true);
              }}
            />
          ) : (
            <>
              {tab === "level" && (
                <LevelView
                  connected={connected}
                  deviceState={noUnit}
                  initialGraph={initialGraph}
                  onCalibrate={onCalibrate}
                  onAddTarget={onAddTarget}
                />
              )}
              {tab === "doctor" && (
                <DoctorView
                  connected={connected}
                  deviceState={noUnit}
                  onCalibrate={onCalibrate}
                />
              )}
              {tab === "copy" && (
                <CopyView
                  connected={connected}
                  deviceState={noUnit}
                  initialGraph={initialGraph}
                />
              )}
              {tab === "songs" && (
                <SongsView connected={connected} deviceState={noUnit} />
              )}
              {tab === "catalog" && <CatalogView />}
              {tab === "settings" && (
                <SettingsView
                  connected={connected}
                  updater={updater}
                  initialCategory={settingsCategory}
                  onReportBug={openBugReport}
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
