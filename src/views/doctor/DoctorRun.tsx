// src/views/doctor/DoctorRun.tsx — the Doctor "Check" run (modal over the select body).
//
// Presentational: useDoctorFlow drives the single `doctor_check` command and streams
// a per-sound status here; this renders the determinate progress + per-sound rows.
// The run is READ-ONLY on the unit (a short test tone through each sound), so its
// scrim is inert (never dismiss a device operation with a stray click). On a natural
// finish it auto-advances to Results, showing a static "done" marker in the footer
// (no CTA competing with the auto-advance) — a manual Stop is confirm-gated, flips
// the footer to "finishing current item…" immediately (the in-flight ~9s capture
// still finishes), and shows a clickable "See results" once it lands. Mirrors
// the Level wizard's RunPage stopping/done footer states. A check that FAILS (the command
// rejected) says so here, with Try again / Back — it never reads as a Stop.

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
import {
  DOCTOR_STEPS,
  countFinished,
  runEnded,
  type DoctorFlowStatus,
  type DoctorRunStatus,
} from "./useDoctorFlow";
import { estimateSecsLeft, avgSoundMs } from "./estimateSecsLeft";
import { useAutoAdvance } from "../../lib/useAutoAdvance";
import type { DoctorInputArg } from "../../lib/types";

/** Rough per-sound check duration (s) — the shrinkage prior before any sound
 *  completes, and still what the header prose quotes ("about 15 seconds
 *  each"). HW-measured: one capture iteration is ~8.5 s capture window +
 *  ~2.8 s settles/gaps + 2 handshakes + the per-preset read ≈ 15 s. */
const SECS_PER_SOUND = 15;

export interface DoctorRunProps {
  /** The run's one status (`useDoctorFlow`'s reducer). `stopping` drives the
   *  "Stopping…" acknowledgment; `failed` is a rejected check, shown as a
   *  failure with a retry — never as a Stop. */
  status: DoctorFlowStatus;
  items: DoctorInputArg[];
  statusByKey: Record<string, DoctorRunStatus>;
  /** The rejection text when `status === "failed"`. */
  error: string | null;
  /** key → instrument display name (null when None — no chip). */
  instName: (key: string) => string | null;
  /** Stop the run (cancels the check; already-checked sounds keep results). */
  onStop: () => void;
  /** Advance to Results (auto after a natural finish, or via Continue). */
  onComplete: () => void;
  /** A failed run: run the same sounds again. */
  onRetry: () => void;
  /** A failed run: back to the preset list. */
  onBack: () => void;
}

export function DoctorRun({
  status,
  items,
  statusByKey,
  error,
  instName,
  onStop,
  onComplete,
  onRetry,
  onBack,
}: DoctorRunProps) {
  const { t } = useTheme();
  const [confirm, setConfirm] = useState(false);
  const done = runEnded(status);
  const stopped = status === "stopped";
  const stopping = status === "stopping";
  const failed = status === "failed";
  const total = items.length;
  const currentIndex = countFinished(status, items, statusByKey);

  // Natural completion auto-advances; a stopped or failed run waits on the user.
  useAutoAdvance(done, stopped || failed, onComplete);

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
    if (failed) return "The check couldn't finish";
    if (stopping) return "Stopping…";
    if (stopped) return "Check stopped";
    if (done) return "Check complete";
    return "Checking your sounds…";
  };

  // Once the run has ended no row is still being listened to: a row the stream
  // left `active` (a failed command, or a stop that landed mid-sound) shows as
  // unfinished, never as a spinner that spins forever.
  const rowStatus = (key: string): DoctorRunStatus | "unfinished" => {
    const s = statusByKey[key] ?? "queued";
    return done && s === "active" ? "unfinished" : s;
  };

  const stateWord = (
    rs: DoctorRunStatus | "unfinished",
  ): { text: string; color: string } => {
    switch (rs) {
      case "unfinished":
        return { text: "didn't finish", color: t.warn };
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
    <WizardShell
      current={1}
      height={560}
      steps={DOCTOR_STEPS}
      label="Doctor run"
    >
      <div
        style={{
          flexShrink: 0,
          padding: `${String(t.space8)}px ${String(t.space10)}px ${String(t.space7)}px`,
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
            margin: `${String(t.space4)}px 0 0`,
            maxWidth: 460,
          }}
        >
          {failed
            ? `Nothing was changed on the unit. ${error ?? ""}`.trim()
            : `Doctor plays a short test tone through each sound and listens back — about ${String(SECS_PER_SOUND)} seconds each. Nothing is changed on the unit.`}
        </div>
        <div
          style={{
            display: "flex",
            alignItems: "baseline",
            justifyContent: "space-between",
            margin: `${String(t.space6)}px 0 ${String(t.space4)}px`,
          }}
        >
          <span style={{ fontFamily: t.mono, fontSize: 12, color: t.ink2 }}>
            {failed
              ? "failed"
              : done
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
          padding: `${String(t.space4)}px ${String(t.space7)}px ${String(t.space3)}px`,
        }}
      >
        {items.map((it) => {
          const rs = rowStatus(it.key);
          const active = rs === "active";
          const inst = instName(it.key);
          const word = stateWord(rs);
          return (
            <RunRow
              key={it.key}
              active={active}
              dim={rs === "queued"}
              statusWidth={96}
              name={it.label}
              tag={it.tag ?? undefined}
              instrument={inst ?? undefined}
              icon={
                <>
                  {active && (
                    <Spinner size={14} stroke={t.sevWarn} strokeWidth={1.8} />
                  )}
                  {rs === "queued" && <Dot color={t.faint} />}
                  {rs === "done" && (
                    <Icon
                      name="check"
                      size={15}
                      stroke={t.good}
                      strokeWidth={2}
                    />
                  )}
                  {(rs === "error" || rs === "unfinished") && (
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

      {confirm && !done ? (
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
          left={
            failed ? (
              <Button
                variant="ghost"
                size="sm"
                onClick={onBack}
                style={{ height: 32, padding: `0 ${String(t.space8)}px` }}
              >
                Back to presets
              </Button>
            ) : (
              <span />
            )
          }
          right={
            failed ? (
              <Button
                variant="primary"
                size="sm"
                icon="refresh"
                onClick={onRetry}
                style={{ height: 32, padding: `0 ${String(t.space8)}px` }}
              >
                Try again
              </Button>
            ) : stopped ? (
              // A manual stop landed — "See results" and Close would do the
              // same thing, so this is the one clickable CTA.
              <Button
                variant="primary"
                size="sm"
                onClick={onComplete}
                style={{ height: 32, padding: `0 ${String(t.space8)}px` }}
              >
                See results
              </Button>
            ) : done ? (
              // Natural finish — a static marker, not a button: useAutoAdvance
              // is already navigating to Results a beat later, so a live CTA
              // here would just be a flash that does the same thing on its own.
              <span
                style={{
                  display: "inline-flex",
                  alignItems: "center",
                  gap: t.space4,
                  fontFamily: t.mono,
                  fontSize: 10.5,
                  letterSpacing: "0.04em",
                  color: t.mutedInk,
                }}
              >
                <Icon name="check" size={13} stroke={t.good} strokeWidth={2} />
                done
              </span>
            ) : stopping ? (
              // Stop already requested — show the wind-down state, not a
              // second Stop button.
              <span
                style={{
                  fontFamily: t.mono,
                  fontSize: 10.5,
                  letterSpacing: "0.04em",
                  color: t.mutedInk,
                }}
              >
                finishing current item…
              </span>
            ) : (
              <Button
                variant="ghost"
                size="sm"
                onClick={() => {
                  setConfirm(true);
                }}
                style={{ height: 32, padding: `0 ${String(t.space8)}px` }}
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
