// src/views/settings/AboutPane.tsx — the "About and updates" page (look B): one Group of
// three rows — "TMP Companion {version}" with "Check for updates" (busy while checking,
// then "Up to date" for a moment), "Install updates by themselves" with its switch, and
// "Something not working?" with "Report a bug" — then the note on where the user's data
// is kept. A found update surfaces through the app-level update toast, not here; a
// failed check goes quietly back to the button.
//
// No update channel (the Linux .deb/.rpm): a check could only ever come back empty, and
// calling that "up to date" would be a lie. So the check and the switch go, and a
// two-line block says the packages update by hand and where to get them.

import { useEffect, useRef, useState } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { Group, Row } from "../../ui/List";
import { Button, Toggle } from "../../ui/primitives";
import { usePlatform } from "../../lib/platform";
import type { UpdaterApi } from "../../lib/useUpdater";
import { SettingsPage } from "./SettingsPage";

/** How long "Up to date" stays before the button comes back. */
const UPTODATE_MS = 2600;

/** Where a Linux user actually gets a newer build. Plain selectable text, not a link:
 *  the app has no external-link capability (no opener plugin), and adding one is a
 *  dependency decision that belongs in its own change. */
const RELEASES_URL =
  "https://github.com/pcavadas/tmp-companion/releases/latest";

export interface AboutPaneProps {
  updater: UpdaterApi;
  /** Opens the Report a bug dialog (the native Help menu opens the same one). */
  onReportBug?: () => void;
}

export function AboutPane({ updater, onReportBug }: AboutPaneProps) {
  const { t } = useTheme();
  const platform = usePlatform();
  const [check, setCheck] = useState<"idle" | "checking" | "uptodate">("idle");
  // Null while `appInfo()` is in flight — treat only an explicit false as "no
  // channel", so the normal rows render during that first tick.
  const noChannel = updater.hasUpdateChannel === false;
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(
    () => () => {
      if (timerRef.current) clearTimeout(timerRef.current);
    },
    [],
  );

  async function runCheck() {
    setCheck("checking");
    const r = await updater.check();
    if (r === "none") {
      setCheck("uptodate");
      timerRef.current = setTimeout(() => {
        setCheck("idle");
      }, UPTODATE_MS);
    } else {
      setCheck("idle");
    }
  }

  const label = { flex: 1, minWidth: 0 } as const;
  // The macOS build is the main one, so an unknown platform reads as a Mac.
  const here =
    platform === null || platform === "macos" ? "this Mac" : "this computer";

  return (
    <SettingsPage title="About and updates">
      <Group label="About and updates">
        <Row>
          <span
            style={{
              ...label,
              fontWeight: t.weightMedium,
              whiteSpace: "nowrap",
              overflow: "hidden",
              textOverflow: "ellipsis",
            }}
          >
            {updater.currentVersion != null
              ? `TMP Companion ${updater.currentVersion}`
              : "TMP Companion"}
          </span>
          {!noChannel &&
            (check === "uptodate" ? (
              <span
                role="status"
                style={{
                  display: "inline-flex",
                  alignItems: "center",
                  gap: t.sp4,
                  fontSize: t.fontCaption,
                  color: t.good,
                  whiteSpace: "nowrap",
                }}
              >
                <Icon name="check" size={12} stroke={t.good} strokeWidth={2} />
                Up to date
              </span>
            ) : (
              <Button
                variant="secondary"
                size="sm"
                busy={check === "checking"}
                onClick={() => {
                  void runCheck();
                }}
              >
                Check for updates
              </Button>
            ))}
        </Row>
        {noChannel ? (
          <div
            style={{
              borderTop: `1px solid ${t.separator}`,
              padding: `${String(t.sp8 + 2)}px ${String(t.sp12)}px`,
              display: "flex",
              flexDirection: "column",
              gap: t.sp4,
              fontSize: t.fontBody,
              lineHeight: `${String(t.lineBody)}px`,
              color: t.text,
            }}
          >
            <span>
              Linux packages update by hand. Download the latest .deb or .rpm
              from the releases page:
            </span>
            <span
              style={{
                fontSize: t.fontCaption,
                lineHeight: `${String(t.lineCaption)}px`,
                userSelect: "text",
                overflowWrap: "anywhere",
              }}
            >
              {RELEASES_URL}
            </span>
          </div>
        ) : (
          <Row>
            <span style={label}>Install updates by themselves</span>
            <Toggle
              on={updater.autoInstall}
              label="Install updates by themselves"
              onClick={() => {
                updater.setAutoInstall(!updater.autoInstall);
              }}
            />
          </Row>
        )}
        {onReportBug && (
          <Row>
            <span style={label}>Something not working?</span>
            <Button variant="secondary" size="sm" onClick={onReportBug}>
              Report a bug
            </Button>
          </Row>
        )}
      </Group>
      <span
        style={{
          fontSize: t.fontCaption,
          lineHeight: `${String(t.lineCaption)}px`,
          color: t.textSecondary,
        }}
      >
        Your instruments and loudness targets are kept on {here}. Presets,
        scenes and songs live on your Tone Master Pro.
      </span>
    </SettingsPage>
  );
}
