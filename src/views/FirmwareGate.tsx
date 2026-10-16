// src/views/FirmwareGate.tsx — the whole-page "untested firmware" notice (a caution
// PageState), shown in place of a unit-driven tab when the connected unit reports
// firmware below FW_MIN. Not a hard block — "Use it anyway" proceeds into the app (App
// holds the per-session override; the toolbar keeps saying "Untested firmware").
// There is no "Check again": while the unit stays connected its version can't change
// (the handshake read it once), and a firmware update restarts the unit, whose reconnect
// re-reads the version and clears this page by itself — the caption says so. There is no
// "how to update" path — updating firmware isn't this app's job.

import { Button } from "../ui/primitives";
import { PageState } from "../ui/PageState";
import { FW_MIN } from "../lib/firmware";

export interface FirmwareGateProps {
  /** The below-floor version the unit reported (e.g. "1.6.3"). */
  detected: string;
  /** Dismiss the notice and proceed into the app for this session. */
  onProceed: () => void;
}

export function FirmwareGate({ detected, onProceed }: FirmwareGateProps) {
  return (
    <PageState
      tone="caution"
      icon="warn-tri"
      title="This firmware hasn't been tested"
      body={`Your Tone Master Pro runs firmware ${detected}. TMP Companion is tested on ${FW_MIN} and later.`}
      actions={
        <Button variant="secondary" onClick={onProceed}>
          Use it anyway
        </Button>
      }
      caption="Updated the firmware? This page checks again when your Tone Master Pro restarts."
    />
  );
}
