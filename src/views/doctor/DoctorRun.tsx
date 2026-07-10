// src/views/doctor/DoctorRun.tsx — the Doctor "Check" run (modal over the select body).
//
// Presentational: useDoctorFlow drives the single `doctor_check` command and streams
// a per-sound status here; this renders the determinate progress + per-sound rows.
// The run is READ-ONLY on the unit (a short test tone through each sound), so its
// scrim is inert (never dismiss a device operation with a stray click). On a natural
// finish it auto-advances to Results; a manual Stop is confirm-gated and keeps every
// already-checked sound's result.

import { useEffect, useState } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Button } from "../../ui/primitives";
import { Icon } from "../../ui/Icon";
import { Spinner } from "../../ui/Spinner";
import { Dot } from "../../ui/Dot";
import { ProgressBar } from "../../ui/ProgressBar";
import { ConfirmBar } from "../../ui/ConfirmBar";
import { RunRow } from "../../ui/RunRow";
import { WizardShell, WizardFooter, WizTitle } from "../overlays/WizardShell";
import { DOCTOR_STEPS, type DoctorRunStatus } from "./useDoctorFlow";
import { estimateSecsLeft, avgSoundMs } from "./estimateSecsLeft";
import { useAutoAdvance } from "../../lib/useAutoAdvance";
import type { DoctorInputArg } from "../../lib/types";

/** Rough per-sound check duration (s) — the shrinkage prior before any sound
 *  completes, and still what the header prose quotes ("about 15 seconds
 *  each"). HW-measured: one capture iteration is ~8.5 s capture window +
 *  ~2.8 s settles/gaps + 2 handshakes + the per-preset read ≈ 15 s. */
const SECS_PER_SOUND = 15;

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

  // Natural completion auto-advances; a stopped run waits for Continue.
  useAutoAdvance(done, stopped, onComplete);

  // Live "about Ns left": a 1 Hz tick plus a completion-timestamp history, so
  // the estimate counts down between sound completions instead of only
  // updating every ~15 s, and the average is a running rate rather than a
  // mean-from-run-start that snaps upward on a slow first sound.
  const [startAt] = useState(() => Date.now());
  const [doneAts, setDoneAts] = useState<number[]>(() => [startAt]);
  const [now, setNow] = useState(startAt);

  // Render-phase adjust: one or more sounds completed since the last render,
  // so record their completion time. Reuses the latest ticked `now` rather
  // than a fresh `Date.now()` — render must stay pure, and `now` is at most
  // one tick (1 s) stale. Uses the OLD `doneAts` below for this render; the
  // re-render with the appended entries follows immediately.
  if (currentIndex > doneAts.length - 1) {
    const next = [...doneAts];
    for (let i = doneAts.length - 1; i < currentIndex; i++) {
      next.push(now);
    }
    setDoneAts(next);
  }

  useEffect(() => {
    if (done) return;
    const id = window.setInterval(() => {
      setNow(Date.now());
    }, 1000);
    return () => {
      window.clearInterval(id);
    };
  }, [done]);

  const stepNo = Math.min(currentIndex + 1, total);
  const pct = total > 0 ? (currentIndex / total) * 100 : 0;
  const avgMs = avgSoundMs(SECS_PER_SOUND * 1000, doneAts);
  const last = doneAts[doneAts.length - 1];
  const secsLeft = estimateSecsLeft(total - currentIndex, avgMs, now - last);

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
            <RunRow
              key={it.key}
              active={active}
              dim={status === "queued"}
              statusWidth={96}
              name={it.label}
              tag={it.tag ?? undefined}
              instrument={inst ?? undefined}
              icon={
                <>
                  {active && (
                    <Spinner size={14} stroke={t.sevWarn} strokeWidth={1.8} />
                  )}
                  {status === "queued" && <Dot color={t.faint} />}
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
                </>
              }
              status={<span style={{ color: word.color }}>{word.text}</span>}
            />
          );
        })}
      </div>

      {confirm ? (
        <ConfirmBar
          message="Stop the check? Checked sounds keep their results."
          onCancel={() => {
            setConfirm(false);
          }}
          onConfirm={() => {
            setConfirm(false);
            onStop();
          }}
        />
      ) : (
        <WizardFooter
          left={<span />}
          right={
            // Read-only check: "See results" and Close would do the same thing,
            // so both Stopped-early and Complete show one primary "See results".
            done ? (
              <Button
                variant="primary"
                small
                onClick={onComplete}
                style={{ height: 32, padding: "0 18px" }}
              >
                See results
              </Button>
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
