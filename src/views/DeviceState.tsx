// src/views/DeviceState.tsx — what a device tab (Level, Doctor, Copy, Songs) shows while
// no unit is connected: ONE page for all four, picked by App's connect state
// (`NoUnitState`, from the last failed connect's typed kind — never its message text,
// which goes to the log only).
//
//   connecting  "Connecting to your Tone Master Pro…" beside a spinner (launch only)
//   off         "Plug in your Tone Master Pro" + Connection help (nothing plugged in, or
//               a unit still starting up)
//   failed      the same page in the danger tint, "Couldn't connect to your Tone Master
//               Pro" + Connection help (any other failure)
//   busy        "Another app is using your Tone Master Pro", worded for the platform, no
//               button (there is nothing to click: quitting the other app is the fix)
//
// Connection is automatic (the 3 s retry and the hotplug events in App), so no page here
// offers a Connect or Scan button. "Connection help" opens ConnectionHelpSheet, owned HERE
// so every no-device page gets a working button without each caller wiring one.

import { useState } from "react";

import { Button } from "../ui/primitives";
import { PageState } from "../ui/PageState";
import type { NoUnitState } from "../lib/connectError";
import { usePlatform } from "../lib/platform";
import { ConnectionHelpSheet } from "./ConnectionHelpSheet";

/** The busy page's body: Fender ships Pro Control for macOS and Windows only, so on Linux
 * the one other app that can hold the unit is a second TMP Companion window. */
const BUSY_BODY = {
  proControl:
    "Quit Pro Control or the other TMP Companion window, and it connects here.",
  linux: "Quit the other TMP Companion window, and it connects here.",
} as const;

export interface DeviceStateProps {
  state: NoUnitState;
}

export function DeviceState({ state }: DeviceStateProps) {
  const platform = usePlatform();
  const [help, setHelp] = useState(false);

  if (state === "connecting")
    return <PageState busy title="Connecting to your Tone Master Pro…" />;

  if (state === "busy")
    return (
      <PageState
        tone="caution"
        icon="warn-tri"
        title="Another app is using your Tone Master Pro"
        body={platform === "linux" ? BUSY_BODY.linux : BUSY_BODY.proControl}
      />
    );

  const failed = state === "failed";
  return (
    <>
      <PageState
        tone={failed ? "danger" : "neutral"}
        icon="usb"
        title={
          failed
            ? "Couldn't connect to your Tone Master Pro"
            : "Plug in your Tone Master Pro"
        }
        body={
          failed
            ? "Unplug it, plug it back in and wait a few seconds. It connects by itself."
            : "Use a USB-C cable that carries data, and switch the Tone Master Pro on. It connects by itself."
        }
        actions={
          <Button
            variant="secondary"
            onClick={() => {
              setHelp(true);
            }}
          >
            Connection help
          </Button>
        }
      />
      {help && (
        <ConnectionHelpSheet
          onClose={() => {
            setHelp(false);
          }}
        />
      )}
    </>
  );
}
