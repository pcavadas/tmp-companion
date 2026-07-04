// src/views/doctor/DoctorRun.tsx — the Doctor "Check" run (modal over the select body).
//
// Presentational: useDoctorFlow drives the single `doctor_check` command and streams
// a per-sound status here; this renders the determinate progress + per-sound rows.
// The run is READ-ONLY on the unit (a short test tone through each sound), so its
// scrim is inert (never dismiss a device operation with a stray click). On a natural
// finish it auto-advances to Results; a manual Stop is confirm-gated and keeps every
// already-checked sound's result.

import { useEffect, useRef, useState } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Button } from "../../ui/primitives";
import { Icon } from "../../ui/Icon";
import { ProgressBar } from "../../ui/ProgressBar";
import { WizardShell, WizardFooter, WizTitle } from "../overlays/WizardShell";
import { DOCTOR_STEPS, type DoctorRunStatus } from "./useDoctorFlow";
import type { DoctorInputArg } from "../../lib/types";

/** Auto-advance delay from a natural completion to the Results page. */
const AUTO_ADVANCE_MS = 650;
/** Rough per-sound check duration (s) — drives the "about Ys left" estimate. */
const SECS_PER_SOUND = 9;

export interface DoctorRunProps {
  items: DoctorInputArg[];
  statusByKey: Record<string, DoctorRunStatus>;
  currentIndex: number;
  total: number;
  done: boolean;
  stopped: boolean;
  /** key → instrument display name (null when None — no chip). */
  instName: (key: string) => string | null;
  /** Stop the run (cancels the check; already-checked sounds keep results). */
  onStop: () => void;
  /** Advance to Results (auto after a natural finish, or via Continue). */
  onComplete: () => void;
}

export function DoctorRun({
  items,
  statusByKey,
  currentIndex,
  total,
  done,
  stopped,
  instName,
  onStop,
  onComplete,
}: DoctorRunProps) {
  const { t } = useTheme();
  const [confirm, setConfirm] = useState(false);

  // Natural completion auto-advances; a stopped run waits for Continue. Read
  // `onComplete` through a ref so a new callback identity doesn't reset the timer.
  const onCompleteRef = useRef(onComplete);
  useEffect(() => {
    onCompleteRef.current = onComplete;
  });
  useEffect(() => {
    if (done && !stopped) {
      const id = window.setTimeout(() => {
        onCompleteRef.current();
      }, AUTO_ADVANCE_MS);
      return () => {
        window.clearTimeout(id);
      };
    }
  }, [done, stopped]);

  const stepNo = Math.min(currentIndex + 1, total);
  const pct = total > 0 ? (currentIndex / total) * 100 : 0;
  const secsLeft = Math.max(0, (total - currentIndex) * SECS_PER_SOUND);

  const headerTitle = (): string => {
    if (stopped) return "Check stopped";
    if (done) return "Check complete";
    return "Checking your sounds…";
  };

  const stateWord = (
    status: DoctorRunStatus,
  ): { text: string; color: string } => {
    switch (status) {
      case "active":
        return { text: "listening…", color: t.sevWarn };
      case "done":
        return { text: "checked", color: t.good };
      case "error":
        return { text: "check failed", color: t.warn };
      default:
        return { text: "queued", color: t.faint };
    }
  };

  return (
    <WizardShell current={1} height={560} steps={DOCTOR_STEPS}>
      <div
        style={{
          flexShrink: 0,
          padding: "16px 24px 14px",
          borderBottom: `0.5px solid ${t.hairline}`,
        }}
      >
        <WizTitle>{headerTitle()}</WizTitle>
        <div
          style={{
            fontFamily: t.sans,
            fontSize: t.fsBody2,
            lineHeight: 1.5,
            color: t.mutedInk,
            margin: "8px 0 0",
            maxWidth: 460,
          }}
        >
          Doctor plays a short test tone through each sound and listens back —
          about {SECS_PER_SOUND} seconds each. Nothing is changed on the unit.
        </div>
        <div
          style={{
            display: "flex",
            alignItems: "baseline",
            justifyContent: "space-between",
            margin: "12px 0 8px",
          }}
        >
          <span style={{ fontFamily: t.mono, fontSize: 12, color: t.ink2 }}>
            {done
              ? stopped
                ? "stopped"
                : "done"
              : `Sound ${String(stepNo)} of ${String(total)}`}
          </span>
          {!done && (
            <span
              style={{ fontFamily: t.mono, fontSize: 10.5, color: t.faint }}
            >
              about {String(secsLeft)}s left
            </span>
          )}
        </div>
        <ProgressBar percent={pct} />
      </div>

      <div
        style={{
          flex: 1,
          minHeight: 0,
          overflowY: "auto",
          overflowX: "hidden",
          padding: "8px 14px 6px",
        }}
      >
        {items.map((it) => {
          const status = statusByKey[it.key] ?? "queued";
          const active = status === "active";
          const inst = instName(it.key);
          const word = stateWord(status);
          return (
            <div
              key={it.key}
              style={{
                padding: "9px 10px",
                borderRadius: 8,
                background: active ? t.accentSoft : "transparent",
              }}
            >
              <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
                <span
                  style={{
                    width: 18,
                    flexShrink: 0,
                    display: "inline-flex",
                    alignItems: "center",
                    justifyContent: "center",
                  }}
                >
                  {active && (
                    <span
                      className="tmp-spin"
                      style={{ display: "inline-flex" }}
                    >
                      <Icon
                        name="spinner"
                        size={14}
                        stroke={t.sevWarn}
                        strokeWidth={1.8}
                      />
                    </span>
                  )}
                  {status === "queued" && (
                    <span
                      style={{
                        width: 7,
                        height: 7,
                        borderRadius: 999,
                        background: t.faint,
                      }}
                    />
                  )}
                  {status === "done" && (
                    <Icon
                      name="check"
                      size={15}
                      stroke={t.good}
                      strokeWidth={2}
                    />
                  )}
                  {status === "error" && (
                    <Icon
                      name="warn-tri"
                      size={14}
                      stroke={t.warn}
                      strokeWidth={1.8}
                    />
                  )}
                </span>
                <span
                  style={{
                    flex: 1,
                    minWidth: 0,
                    display: "flex",
                    alignItems: "baseline",
                    gap: 8,
                  }}
                >
                  <span
                    style={{
                      fontFamily: t.serif,
                      fontSize: 14.5,
                      color: status === "queued" ? t.mutedInk : t.ink,
                      whiteSpace: "nowrap",
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                    }}
                  >
                    {it.label}
                  </span>
                  {it.tag && (
                    <span
                      style={{
                        fontFamily: t.mono,
                        fontSize: 8.5,
                        letterSpacing: "0.04em",
                        color: t.accentDeep,
                        flexShrink: 0,
                      }}
                    >
                      {it.tag}
                    </span>
                  )}
                </span>
                {inst && (
                  <span
                    style={{
                      fontFamily: t.mono,
                      fontSize: 10.5,
                      color: t.mutedInk,
                      border: `0.5px solid ${t.hairlineStrong}`,
                      borderRadius: 5,
                      padding: "2px 7px",
                      flexShrink: 0,
                    }}
                  >
                    {inst}
                  </span>
                )}
                <span
                  style={{
                    fontFamily: t.mono,
                    fontSize: 11,
                    flexShrink: 0,
                    width: 96,
                    whiteSpace: "nowrap",
                    textAlign: "right",
                    color: word.color,
                  }}
                >
                  {word.text}
                </span>
              </div>
            </div>
          );
        })}
      </div>

      {confirm ? (
        <div
          style={{
            flexShrink: 0,
            borderTop: `0.5px solid ${t.hairline}`,
            padding: "13px 22px",
            background: t.bgAlt,
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            gap: 14,
          }}
        >
          <span style={{ fontFamily: t.sans, fontSize: 12.5, color: t.ink2 }}>
            Stop the check? Sounds already checked keep their results — safe to
            stop anytime.
          </span>
          <div style={{ display: "flex", gap: 9 }}>
            <Button
              variant="ghost"
              small
              onClick={() => {
                setConfirm(false);
              }}
              style={{ height: 30, padding: "0 13px" }}
            >
              Keep going
            </Button>
            <Button
              variant="warn"
              small
              onClick={() => {
                setConfirm(false);
                onStop();
              }}
              style={{ height: 30, padding: "0 14px" }}
            >
              Stop
            </Button>
          </div>
        </div>
      ) : (
        <WizardFooter
          left={<span />}
          right={
            stopped ? (
              <Button
                variant="primary"
                small
                onClick={onComplete}
                style={{ height: 32, padding: "0 18px" }}
              >
                Continue
              </Button>
            ) : done ? (
              <span
                style={{
                  display: "inline-flex",
                  alignItems: "center",
                  gap: 7,
                  fontFamily: t.mono,
                  fontSize: 10.5,
                  letterSpacing: "0.04em",
                  color: t.mutedInk,
                }}
              >
                <Icon name="check" size={13} stroke={t.good} strokeWidth={2} />
                done
              </span>
            ) : (
              <Button
                variant="ghost"
                small
                onClick={() => {
                  setConfirm(true);
                }}
                style={{ height: 32, padding: "0 15px" }}
              >
                Stop
              </Button>
            )
          }
        />
      )}
    </WizardShell>
  );
}

export default DoctorRun;
