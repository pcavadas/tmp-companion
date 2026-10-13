// src/views/LibraryScanErrorBanner.tsx — the app-wide notice for a FAILED library
// scan (device/libraryScan). A failed backup read leaves every backup-sourced map
// empty, which Level, Doctor, Copy and Songs would otherwise show as "no scenes, no
// footswitches, no songs". This banner says the read failed instead, under the tab
// bar on every tab, with a Retry that runs the scan again. Subscribes to the scan
// store itself so App doesn't re-render on scan changes.

import { useSyncExternalStore } from "react";

import { useTheme } from "../theme/ThemeContext";
import { AlertBanner, Button } from "../ui/primitives";
import {
  getLibraryScan,
  retryLibraryScan,
  subscribeLibraryScan,
} from "../device/libraryScan";

export function LibraryScanErrorBanner() {
  const { t } = useTheme();
  const lib = useSyncExternalStore(subscribeLibraryScan, getLibraryScan);
  if (lib.error == null) return null;
  return (
    <AlertBanner
      style={{
        margin: `${String(t.space5)}px ${String(t.space8)}px 0`,
        padding: `${String(t.space4)}px ${String(t.space6)}px`,
        fontSize: t.fsUi,
        flexShrink: 0,
        display: "flex",
        alignItems: "center",
        gap: t.space6,
      }}
    >
      <span style={{ flex: 1 }}>
        Couldn&rsquo;t read your preset library from the unit, so scenes,
        footswitches, signal paths and songs may be missing here. ({lib.error})
      </span>
      <Button
        variant="ghost"
        size="sm"
        icon="refresh"
        onClick={retryLibraryScan}
      >
        Retry
      </Button>
    </AlertBanner>
  );
}
