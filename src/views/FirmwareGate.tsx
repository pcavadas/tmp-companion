// src/views/FirmwareGate.tsx — full-page "untested firmware" notice.
//
// Warn-toned sibling of EmptyState: shown in place of a unit-driven tab body
// when the connected unit reports firmware below FW_MIN. Not a hard block —
// "Use it anyway" proceeds into the app (App holds the per-session override),
// "Check again" re-reads the unit's firmware in case it was just updated. There
// is no "how to update" path — updating firmware isn't this app's job.

import { useTheme } from "../theme/ThemeContext";
import { Icon } from "../ui/Icon";
import { PageNotice } from "./PageNotice";
import { FW_MIN } from "../lib/firmware";

export interface FirmwareGateProps {
  /** The below-floor version the unit reported (e.g. "1.6.3"). */
  detected: string;
  /** Re-read the unit's firmware. */
  onCheckAgain: () => void;
  /** Dismiss the notice and proceed into the app for this session. */
  onProceed: () => void;
}

export function FirmwareGate({
  detected,
  onCheckAgain,
  onProceed,
}: FirmwareGateProps) {
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
          TMP Companion has only been tested with firmware {FW_MIN} and later.
          Your Tone Master Pro is running{" "}
          <span style={{ fontFamily: t.mono, color: t.warn }}>{detected}</span>{" "}
          — leveling and other features may not behave as expected.
        </>
      }
      bodyMaxWidth={440}
      primary={{ label: "Check again", icon: "refresh", onClick: onCheckAgain }}
      secondary={{ label: "Use it anyway", onClick: onProceed }}
    />
  );
}
