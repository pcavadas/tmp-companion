// src/views/doctor/DoctorRun.tsx — Doctor's Check page: a page under the toolbar while the
// check listens (look B, like Level's run).
//
// Top to bottom: the title ("Checking 3 sounds"; "Stopping…" after a Stop; "The check
// couldn't finish" when the command failed) with Doctor's Steps; the now card — on the
// left "Listening to Tweed Warm. Nothing changes on your Tone Master Pro.", on the right
// the progress bar and "About 30 seconds left"; one Group with a compact row per sound
// (status mark, preset name, sound name, state word); and the Footer with Stop on the left.
//
// Presentational: `useDoctorFlow` drives the one `doctor_check` command and streams each
// sound's status here. The check only listens, so nothing on the unit changes. Stop asks
// first in a dialog that only its own buttons close; the check keeps going while it is
// open. After a Stop the page says it is finishing the sound in progress, then Results
// open by themselves — and a natural finish shows "All 3 sounds checked." for a beat
// before Results. A failed check says so with Back and Try again; its error text goes to
// the log, never on screen.

import { useEffect, useState } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Footer } from "../../ui/Footer";
import { Group, Row } from "../../ui/List";
import { ProgressBar } from "../../ui/ProgressBar";
import { Button, Modal } from "../../ui/primitives";
import { useAutoAdvance } from "../../lib/useAutoAdvance";
import { useWindowWidth } from "../../lib/useWindowWidth";
import type { DoctorInputArg } from "../../lib/types";
import { FlowPage } from "../level/FlowPage";
import { StatusMark } from "../level/StatusMark";
import {
  countFinished,
  runEnded,
  type DoctorFlowStatus,
  type DoctorRunStatus,
} from "./useDoctorFlow";
import { avgSoundMs, estimateSecsLeft, timeLeftText } from "./estimateSecsLeft";
import { checkNowLine, checkRowState, type RunName } from "./runRows";
import { WIDE_WINDOW } from "./layout";

/** Rough per-sound check duration (s) — the shrinkage prior before any sound
 *  completes. HW-measured: one capture iteration is ~8.5 s capture window +
 *  ~2.8 s settles/gaps + 2 handshakes + the per-preset read ≈ 15 s. */
const SECS_PER_SOUND = 15;

export interface DoctorRunProps {
  /** The run's one status (`useDoctorFlow`'s reducer). `stopping` drives "Stopping…";
   *  `failed` is a rejected check, shown as a failure with Try again — never as a Stop. */
  status: DoctorFlowStatus;
  items: DoctorInputArg[];
  statusByKey: Record<string, DoctorRunStatus>;
  /** Each sound's preset and sound name, by key. */
  nameOf: (key: string) => RunName;
  /** Doctor's steps for this check (the Instrument page may have been skipped). */
  steps: readonly string[];
  /** Stop the check (already-checked sounds keep their results). */
  onStop: () => void;
  /** On to Results (by itself, after a finish or a Stop). */
  onComplete: () => void;
  /** A failed check: the same sounds again. */
  onRetry: () => void;
  /** A failed check: back to the preset list. */
  onBack: () => void;
}

export function DoctorRun({
  status,
  items,
  statusByKey,
  nameOf,
  steps,
  onStop,
  onComplete,
  onRetry,
  onBack,
}: DoctorRunProps) {
  const { t } = useTheme();
  const wide = useWindowWidth() >= WIDE_WINDOW;
  const [asking, setAsking] = useState(false);
  const over = runEnded(status);
  const failed = status === "failed";
  const stopping = status === "stopping";
  const total = items.length;
  const finished = countFinished(status, items, statusByKey);

  // A finish or a Stop goes on to Results by itself; a failure waits on the user.
  useAutoAdvance(over && !failed, false, onComplete);

  // Live "About N seconds left": a 1 Hz tick plus a completion-timestamp history, so
  // the estimate counts down between sound completions instead of only updating every
  // ~15 s, and the average is a running rate rather than a mean-from-run-start that
  // snaps upward on a slow first sound.
  const [startAt] = useState(() => Date.now());
  const [doneAts, setDoneAts] = useState<number[]>(() => [startAt]);
  const [now, setNow] = useState(startAt);

  // Render-phase adjust: one or more sounds completed since the last render, so record
  // their completion time. Reuses the latest ticked `now` rather than a fresh
  // `Date.now()` — render must stay pure, and `now` is at most one tick (1 s) stale.
  if (finished > doneAts.length - 1) {
    const next = [...doneAts];
    for (let i = doneAts.length - 1; i < finished; i++) next.push(now);
    setDoneAts(next);
  }

  useEffect(() => {
    if (over) return;
    const id = window.setInterval(() => {
      setNow(Date.now());
    }, 1000);
    return () => {
      window.clearInterval(id);
    };
  }, [over]);

  const avgMs = avgSoundMs(SECS_PER_SOUND * 1000, doneAts);
  const last = doneAts[doneAts.length - 1];
  const secsLeft = estimateSecsLeft(total - finished, avgMs, now - last);

  const title = failed
    ? "The check couldn't finish"
    : stopping || status === "stopped"
      ? "Stopping…"
      : `Checking ${String(total)} ${total === 1 ? "sound" : "sounds"}`;

  const secondary = {
    fontSize: t.fontCaption,
    lineHeight: `${String(t.lineCaption)}px`,
    color: t.textSecondary,
    whiteSpace: "nowrap",
  } as const;

  return (
    <FlowPage
      title={title}
      step={steps.length - 2}
      steps={steps}
      label="Checking sounds"
      maxWidth={wide ? 720 : undefined}
      fill
      footer={
        <Footer
          left={
            failed ? (
              <Button variant="secondary" size="lg" onClick={onBack}>
                Back
              </Button>
            ) : (
              <Button
                variant="secondary"
                size="lg"
                icon="stop"
                busy={stopping}
                disabled={over}
                onClick={() => {
                  setAsking(true);
                }}
              >
                Stop
              </Button>
            )
          }
          right={
            failed && (
              <Button variant="primary" size="lg" onClick={onRetry}>
                Try again
              </Button>
            )
          }
        />
      }
    >
      <div
        style={{
          flex: 1,
          minHeight: 0,
          display: "flex",
          flexDirection: "column",
          gap: t.sp12 + t.sp2,
        }}
      >
        <div
          style={{
            flexShrink: 0,
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            gap: t.sp16,
            padding: `${String(t.sp12 + t.sp2)}px ${String(t.sp16)}px`,
            background: t.surface,
            border: `1px solid ${t.border}`,
            borderRadius: t.radiusGroup,
          }}
        >
          <span
            aria-live="polite"
            style={{
              fontSize: t.fontBody,
              lineHeight: `${String(t.lineBody)}px`,
              minWidth: 0,
            }}
          >
            {checkNowLine(status, items, statusByKey, nameOf)}
          </span>
          {!failed && (
            <span
              style={{
                flexShrink: 0,
                display: "flex",
                alignItems: "center",
                gap: t.sp8 + t.sp2,
              }}
            >
              <span
                role="progressbar"
                aria-label="Sounds checked"
                aria-valuemin={0}
                aria-valuemax={total}
                aria-valuenow={finished}
                style={{ width: 120 }}
              >
                <ProgressBar
                  percent={total > 0 ? (finished / total) * 100 : 0}
                />
              </span>
              {!over && !stopping && (
                <span style={secondary}>{timeLeftText(secsLeft)}</span>
              )}
            </span>
          )}
        </div>

        <div style={{ flex: "0 1 auto", minHeight: 0, overflowY: "auto" }}>
          <Group label="Sounds being checked">
            {items.map((it) => {
              const state = checkRowState(status, statusByKey[it.key]);
              const n = nameOf(it.key);
              return (
                <Row key={it.key} size="compact" data-check-row={it.key}>
                  <StatusMark kind={state.mark} />
                  <span
                    style={{
                      flex: 1,
                      minWidth: 0,
                      whiteSpace: "nowrap",
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      fontWeight: t.weightMedium,
                    }}
                  >
                    {n.preset}{" "}
                    <span
                      style={{
                        color: t.textSecondary,
                        fontWeight: t.weightRegular,
                      }}
                    >
                      {n.sound}
                    </span>
                  </span>
                  <span
                    style={{
                      ...secondary,
                      color:
                        state.tone === "good"
                          ? t.good
                          : state.tone === "text"
                            ? t.text
                            : t.textSecondary,
                    }}
                  >
                    {state.word}
                  </span>
                </Row>
              );
            })}
          </Group>
        </div>
      </div>

      <Modal
        open={asking && !over}
        label="Stop checking?"
        headline="Stop checking?"
        body="Sounds checked so far keep their results."
        cancelLabel="Keep checking"
        applyLabel="Stop"
        onCancel={() => {
          setAsking(false);
        }}
        onApply={() => {
          setAsking(false);
          onStop();
        }}
      />
    </FlowPage>
  );
}
