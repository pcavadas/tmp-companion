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

interface AppUpdatesSectionProps {
  updater: UpdaterApi;
}

export function AppUpdatesSection({ updater }: AppUpdatesSectionProps) {
  const { t } = useTheme();
  const s = useStyles();
  const [check, setCheck] = useState<"idle" | "checking" | "uptodate">("idle");
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
      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
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

        {check === "idle" && (
          <Button
            variant="ghost"
            small
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
              gap: 6,
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
              gap: 5,
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
            gap: 10,
            marginTop: 4,
          }}
        >
          <span style={{ fontFamily: t.sans, fontSize: t.fsUi, color: t.ink2 }}>
            Install updates automatically
          </span>
          <Toggle
            on={updater.autoInstall}
            onClick={() => {
              updater.setAutoInstall(!updater.autoInstall);
            }}
          />
        </div>
      </div>
    </div>
  );
}
