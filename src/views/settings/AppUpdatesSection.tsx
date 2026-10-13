// src/views/settings/AppUpdatesSection.tsx — Settings tab "Software update" section:
// version readout + manual check (checking… → up to date, self-clears) + the
// auto-install toggle. The "found" result surfaces via the app-level update
// toast, not here.

import { useEffect, useRef, useState } from "react";

import { DASH } from "../../lib/format";
import { useTheme, useStyles } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { Spinner } from "../../ui/Spinner";
import { Button, Toggle } from "../../ui/primitives";
import type { UpdaterApi } from "../../lib/useUpdater";

const UPTODATE_MS = 2600;

/** Where a Linux user actually gets a newer build. Plain text, not a link: the
 *  app has no external-link capability (no opener plugin), and adding one is a
 *  dependency decision that belongs in its own change. */
const RELEASES_URL =
  "https://github.com/pcavadas/tmp-companion/releases/latest";

interface AppUpdatesSectionProps {
  updater: UpdaterApi;
}

export function AppUpdatesSection({ updater }: AppUpdatesSectionProps) {
  const { t } = useTheme();
  const s = useStyles();
  const [check, setCheck] = useState<"idle" | "checking" | "uptodate">("idle");
  // Null while `appInfo()` is in flight — treat only an explicit false as "no
  // channel", so the normal UI is what renders during that first tick.
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

  return (
    <div>
      <div style={s.kicker(t.accentDeep)}>Software update</div>
      <div style={{ display: "flex", flexDirection: "column", gap: t.space4 }}>
        <div
          style={{
            fontFamily: t.serif,
            fontSize: t.fsBody2,
            color: t.ink,
            whiteSpace: "nowrap",
          }}
        >
          Version {updater.currentVersion ?? DASH}
        </div>

        {/* No update channel (Linux): the check can only ever come back
            empty, and reporting that as "up to date" is a lie — the .deb/.rpm
            really are updated by hand. Say so, and drop the two affordances
            that cannot work here. */}
        {noChannel ? (
          <div
            style={{
              fontFamily: t.sans,
              fontSize: t.fsUi,
              color: t.ink2,
              lineHeight: 1.5,
            }}
          >
            Linux packages update by downloading the latest .deb or .rpm from
            the project’s releases page:
            <div
              style={{
                fontFamily: t.mono,
                fontSize: t.fsData2,
                color: t.ink,
                marginTop: t.space2,
                userSelect: "text",
                overflowWrap: "anywhere",
              }}
            >
              {RELEASES_URL}
            </div>
          </div>
        ) : (
          <>
            {check === "idle" && (
              <Button
                variant="ghost"
                size="sm"
                onClick={() => {
                  void runCheck();
                }}
              >
                Check for updates
              </Button>
            )}
            {check === "checking" && (
              <div
                style={{
                  display: "inline-flex",
                  alignItems: "center",
                  gap: t.space3,
                  fontFamily: t.mono,
                  fontSize: t.fsData2,
                  color: t.sevWarn,
                }}
              >
                <Spinner
                  name="refresh"
                  size={12}
                  stroke={t.sevWarn}
                  strokeWidth={2}
                />
                checking…
              </div>
            )}
            {check === "uptodate" && (
              <div
                style={{
                  display: "inline-flex",
                  alignItems: "center",
                  gap: t.space3,
                  fontFamily: t.mono,
                  fontSize: t.fsData2,
                  color: t.good,
                }}
              >
                <Icon name="check" size={11} stroke={t.good} strokeWidth={2} />
                up to date
              </div>
            )}

            <div
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                gap: t.space5,
                marginTop: t.space2,
              }}
            >
              <span
                style={{ fontFamily: t.sans, fontSize: t.fsUi, color: t.ink2 }}
              >
                Install updates automatically
              </span>
              <Toggle
                on={updater.autoInstall}
                label="Install updates automatically"
                onClick={() => {
                  updater.setAutoInstall(!updater.autoInstall);
                }}
              />
            </div>
          </>
        )}
      </div>
    </div>
  );
}
