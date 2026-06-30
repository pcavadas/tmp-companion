import { useEffect, useState } from "react";
import { useTheme } from "../theme/ThemeContext";
import { Icon } from "./Icon";
import { FW_MIN, firmwareSupported } from "../lib/firmware";

/** Minimum time the "reading firmware…" phase stays visible. The version is
 * already in hand when `connected` flips (connect_device resolves with it), so
 * this floor is purely UX — the handshake read shouldn't flash by. */
const MIN_READING_MS = 900;

export interface DeviceStatusProps {
  connected: boolean;
  /** Firmware the connected unit reported (e.g. "1.7.75"); null when unknown. */
  firmwareVersion: string | null;
}

type Phase = "off" | "reading" | "ready";

/**
 * Device-status indicator (top-right of the tab bar, every page). The DOT
 * carries the link state — green = unit present, amber pulsing = reading it,
 * hollow = no unit — and the LABEL carries the most useful thing the link
 * tells us: the firmware the connected unit is running (`labeled` format from
 * the design handoff: `connected · 1.7.75`). Passive readout — no click target.
 */
export function DeviceStatus({
  connected,
  firmwareVersion,
}: DeviceStatusProps) {
  const { t } = useTheme();

  // Detection handshake: re-runs on every (re)connect — off → reading → ready.
  // `phase` derives from `connected` + whether the min-reading window elapsed for
  // the current connection. The elapsed flag is reset on a connection change via
  // React's "adjust state during render" pattern (not in an effect), and the
  // effect only flips it from inside the timer — never synchronously.
  const [minElapsed, setMinElapsed] = useState(false);
  const [prevConnected, setPrevConnected] = useState(connected);
  if (connected !== prevConnected) {
    setPrevConnected(connected);
    setMinElapsed(false);
  }
  useEffect(() => {
    if (!connected) return;
    const id = setTimeout(() => {
      setMinElapsed(true);
    }, MIN_READING_MS);
    return () => {
      clearTimeout(id);
    };
  }, [connected]);
  const phase: Phase = !connected ? "off" : minElapsed ? "ready" : "reading";

  // Connected to a unit running firmware below the tested floor: the dot + label
  // turn warn-toned and a static ⚠ trails the version — a persistent reminder
  // that survives "use it anyway" (the gate notice itself lives in FirmwareGate).
  const untested = phase === "ready" && !firmwareSupported(firmwareVersion);

  const dotColor =
    phase === "ready"
      ? untested
        ? t.warn
        : t.good
      : phase === "reading"
        ? t.sevWarn
        : "transparent";

  let label: React.ReactNode;
  if (phase === "off") {
    label = <span style={{ color: t.faint }}>disconnected</span>;
  } else if (phase === "reading") {
    label = <span style={{ color: t.faint }}>reading firmware…</span>;
  } else if (untested) {
    label = (
      <>
        <span style={{ color: t.warn }}>untested</span>
        {firmwareVersion && (
          <span
            style={{ color: t.warn, fontVariantNumeric: "tabular-nums" }}
          >{` · ${firmwareVersion}`}</span>
        )}
        <Icon name="warn-tri" size={12} stroke={t.warn} />
      </>
    );
  } else {
    label = (
      <>
        <span style={{ color: t.mutedInk }}>connected</span>
        {firmwareVersion && (
          <span
            style={{ color: t.faint, fontVariantNumeric: "tabular-nums" }}
          >{` · ${firmwareVersion}`}</span>
        )}
      </>
    );
  }

  const title =
    phase === "ready"
      ? firmwareVersion
        ? untested
          ? `Tone Master Pro · firmware ${firmwareVersion} — untested below ${FW_MIN}`
          : `Tone Master Pro · firmware ${firmwareVersion}`
        : "Tone Master Pro"
      : phase === "reading"
        ? "Reading firmware from unit…"
        : "No unit connected";

  return (
    <span
      title={title}
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 7,
        fontFamily: t.mono,
        fontSize: t.fsMeta,
        letterSpacing: t.lsMeta,
        whiteSpace: "nowrap",
      }}
    >
      <span
        className={phase === "reading" ? "tmp-fwpulse" : undefined}
        style={{
          width: 7,
          height: 7,
          borderRadius: t.rPill,
          boxSizing: "border-box",
          background: dotColor,
          border: phase === "off" ? `1px solid ${t.faint}` : "none",
        }}
      />
      {label}
    </span>
  );
}
