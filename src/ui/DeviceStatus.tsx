// src/ui/DeviceStatus.tsx — the toolbar's device readout (look B): a Dot (or a spinner)
// and one short phrase, 12 px secondary text, `role="status"`. A passive readout with no
// click target: connection is fully automatic, so there are no Connect/Disconnect
// controls anywhere.
//
//   connecting  spinner + "Connecting…" (also the first 900 ms after every (re)connect)
//   connected   good dot + "Connected · firmware {v}" ("Connected" when the unit gave none)
//   untested    caution dot + "Untested firmware {v}" in caution (stays after "Use it anyway")
//   busy        caution dot + "In use by another app"
//   off         hollow dot + "Not connected" (also a connect that failed for another reason)

import { useEffect, useState } from "react";

import { useTheme } from "../theme/ThemeContext";
import { Dot } from "./Dot";
import { Spinner } from "./Spinner";

/** Minimum time "Connecting…" stays up after a connect lands. The version is already in
 * hand when the unit connects (connect_device resolves with it), so this floor is purely
 * UX — the handshake read shouldn't flash by. */
const MIN_READING_MS = 900;

export type DeviceStatusState =
  "connecting" | "connected" | "untested" | "busy" | "off";

export interface DeviceStatusProps {
  state: DeviceStatusState;
  /** Firmware the connected unit reported (e.g. "1.8.45"); null when unknown. */
  firmwareVersion: string | null;
}

export function DeviceStatus({ state, firmwareVersion }: DeviceStatusProps) {
  const { t } = useTheme();

  // Every (re)connect passes through "Connecting…" for the minimum window. The elapsed
  // flag resets when the link comes up, via React's "adjust state during render"
  // pattern (not in an effect), and only the timer flips it back.
  const live = state === "connected" || state === "untested";
  const [minElapsed, setMinElapsed] = useState(false);
  const [prevLive, setPrevLive] = useState(live);
  if (live !== prevLive) {
    setPrevLive(live);
    setMinElapsed(false);
  }
  useEffect(() => {
    if (!live) return;
    const id = setTimeout(() => {
      setMinElapsed(true);
    }, MIN_READING_MS);
    return () => {
      clearTimeout(id);
    };
  }, [live]);
  const shown: DeviceStatusState = live && !minElapsed ? "connecting" : state;

  let mark: React.ReactNode;
  let words: React.ReactNode;
  switch (shown) {
    case "connecting":
      mark = <Spinner size={11} stroke={t.textSecondary} strokeWidth={2.4} />;
      words = "Connecting…";
      break;
    case "connected":
      mark = <Dot tone="good" />;
      words =
        firmwareVersion != null
          ? `Connected · firmware ${firmwareVersion}`
          : "Connected";
      break;
    case "untested":
      mark = <Dot tone="caution" />;
      words = (
        <span style={{ color: t.caution }}>
          {firmwareVersion != null
            ? `Untested firmware ${firmwareVersion}`
            : "Untested firmware"}
        </span>
      );
      break;
    case "busy":
      mark = <Dot tone="caution" />;
      words = "In use by another app";
      break;
    case "off":
      mark = <Dot tone="neutral" hollow />;
      words = "Not connected";
      break;
  }

  return (
    <div
      role="status"
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: t.sp6,
        fontFamily: t.fontSans,
        fontSize: t.fontCaption,
        lineHeight: `${String(t.lineCaption)}px`,
        color: t.textSecondary,
        fontVariantNumeric: "tabular-nums",
        whiteSpace: "nowrap",
      }}
    >
      {mark}
      {words}
    </div>
  );
}
