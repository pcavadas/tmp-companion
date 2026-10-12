// src/views/FirmwareGate.tsx — full-page "untested firmware" notice.
//
// Warn-toned sibling of EmptyState: shown in place of a unit-driven tab body
// when the connected unit reports firmware below FW_MIN. Not a hard block —
// "Use it anyway" proceeds into the app (App holds the per-session override).
// There is no "Check again": while the unit stays connected its version can't
// change (the handshake read it once), and a firmware update restarts the unit,
// whose reconnect re-reads the version and clears this page on its own — the
// hint says so. There is no "how to update" path — updating firmware isn't this
// app's job.

import { useTheme } from "../theme/ThemeContext";
import { Icon } from "../ui/Icon";
import { PageNotice } from "./PageNotice";
import { FW_MIN } from "../lib/firmware";

export interface FirmwareGateProps {
  /** The below-floor version the unit reported (e.g. "1.6.3"). */
  detected: string;
  /** Dismiss the notice and proceed into the app for this session. */
  onProceed: () => void;
}

export function FirmwareGate({ detected, onProceed }: FirmwareGateProps) {
  const { t } = useTheme();
  return (
    <PageNotice
      tone="warn"
      medallion={
        <Icon name="warn-tri" size={30} stroke={t.warn} strokeWidth={1.5} />
      }
      kicker="Untested firmware"
      title="This firmware hasn’t been tested"
      body={
        <>
          TMP Companion is tested only on firmware {FW_MIN} and later. Your Tone
          Master Pro is running{" "}
          <span style={{ fontFamily: t.mono, color: t.warn }}>{detected}</span>.
          Leveling and other features may not work right.
        </>
      }
      bodyMaxWidth={440}
      secondary={{ label: "Use it anyway", onClick: onProceed }}
      hint="Updated the firmware? This page checks again on its own when the unit restarts and reconnects."
    />
  );
}
