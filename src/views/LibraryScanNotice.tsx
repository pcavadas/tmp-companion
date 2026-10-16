// src/views/LibraryScanNotice.tsx — the app-wide notice for a FAILED library scan
// (device/libraryScan). A failed backup read leaves every backup-sourced map empty, which
// Level, Doctor, Copy and Songs would otherwise show as "no scenes, no footswitches, no
// songs". This caution Notice says the read failed instead — once, directly under the
// toolbar on every tab while connected, never scrolling with the content — with a "Try
// again" that runs the scan again (the Notice hides while it runs, and comes back if it
// fails again). The failure's own text is never drawn: it is in the log, and so in a bug
// report. Subscribes to the scan store itself so App doesn't re-render on scan changes.

import { useSyncExternalStore } from "react";

import { useTheme } from "../theme/ThemeContext";
import { Button, Notice } from "../ui/primitives";
import {
  getLibraryScan,
  retryLibraryScan,
  subscribeLibraryScan,
} from "../device/libraryScan";

export function LibraryScanNotice() {
  const { t } = useTheme();
  const lib = useSyncExternalStore(subscribeLibraryScan, getLibraryScan);
  if (lib.error == null) return null;
  return (
    <Notice
      tone="caution"
      action={
        <Button variant="secondary" size="sm" onClick={retryLibraryScan}>
          Try again
        </Button>
      }
      style={{
        margin: `${String(t.sp12)}px ${String(t.sp16)}px 0`,
        padding: `${String(t.sp8 + t.sp2)}px ${String(t.sp12)}px`,
        flexShrink: 0,
      }}
    >
      Couldn&apos;t read your preset details, so scenes, footswitches, signal
      paths and songs are missing here.
    </Notice>
  );
}
