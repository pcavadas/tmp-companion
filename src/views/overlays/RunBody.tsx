// src/views/overlays/RunBody.tsx — wizard step 3, "Level" (running).
//
// Presentational: the useLevelingFlow hook drives the sequence (one chosen scene at a
// time, loading each on the unit, measuring, adjusting, saving) and updates the items'
// live status/outcome here. Per-step status: queued · active ("connecting…", then the live
// readout owns the cell once a capture streams) · result
// (done · −24.0 / clamped · −25.8 / not on USB 1/2 / skipped · read failed).
//
// Completion: when the run finishes on its OWN it auto-advances to Summary after 650ms,
// showing a static "✓ done" marker in the footer (no flashing Continue button). A
// Continue button appears only when the user manually STOPPED the run. Cancel opens an
// inline confirm that replaces the footer.

import { useState } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Button } from "../../ui/primitives";
import { Icon } from "../../ui/Icon";
import { Spinner } from "../../ui/Spinner";
import { Dot } from "../../ui/Dot";
import { ProgressBar } from "../../ui/ProgressBar";
import { LiveVU } from "../../ui/LiveVU";
import { LiveReadout } from "../../ui/LiveReadout";
import { ConfirmBar } from "../../ui/ConfirmBar";
import { RunRow } from "../../ui/RunRow";
import { WizardFooter, WizTitle } from "./WizardShell";
import { fmtLufs } from "../../lib/format";
import { useAutoAdvance } from "../../lib/useAutoAdvance";
import type { RunItem } from "../level/leveling";

export interface RunBodyProps {
  items: RunItem[];
  currentIndex: number;
  total: number;
  done: boolean;
  stopped: boolean;
  /** Stop requested; the in-flight item is winding down (no row is truly idle yet). */
  stopping: boolean;
  /** Advisory live measured loudness for the active row's "measuring…" readout (null =
   *  nothing streaming). Reference-level, NOT the final value — the result row is the confirm. */
  liveLufs: number | null;
  /** Rolling per-hop momentary levels (dB, newest last) driving the live VU bars. */
  liveTrace: number[];
  /** Resolve an instrument profile id to its display name (the per-row chip). */
  instrumentName: (id: string) => string;
  /** Stop the run (sets the cancel flag; the loop publishes done+stopped). */
  onCancel: () => void;
  /** Advance to the Summary step (auto after a natural finish, or via Continue). */
  onComplete: () => void;
}

export function RunBody({
  items,
  currentIndex,
  total,
  done,
  stopped,
  stopping,
  liveLufs,
  liveTrace,
  instrumentName,
  onCancel,
  onComplete,
}: RunBodyProps) {
  const { t } = useTheme();
  const [confirm, setConfirm] = useState(false);

  // Natural completion auto-advances; a stopped run waits for Continue.
  useAutoAdvance(done, stopped, onComplete);

  const stepNo = Math.min(currentIndex + 1, total);
  // currentIndex reaches `total` on a natural finish (→ 100%) and stays partial on a
  // stop, so the bare ratio covers every case — no done/stopped branching needed.
  const pct = total > 0 ? (currentIndex / total) * 100 : 0;

  const resultText = (it: RunItem): string => {
    if (it.outcome === "clamped") return `clamped · ${fmtLufs(it.value)}`;
    if (it.outcome === "offbranch") return "not on USB 1/2";
    if (it.outcome === "skipped") return "skipped · read failed";
    return `done · ${fmtLufs(it.value)}`;
  };
  const resultColor = (it: RunItem): string =>
    it.outcome === "clamped"
      ? t.sevWarn
      : it.outcome === "offbranch"
        ? t.warn
        : it.outcome === "skipped"
          ? t.mutedInk
          : t.good;
  const headerTitle = (): string => {
    if (stopping) return "Stopping…";
    if (stopped) return "Leveling stopped";
    if (done) return "Leveling complete";
    return "Leveling…";
  };
  const rowStatus = (it: RunItem): string => {
    // Active-but-not-yet-streaming = loading the preset + engaging re-amp (no LUFS events
    // yet). "connecting…" is truer than "leveling…" for that pre-capture window; once the
    // capture streams, `live !== null` hides this cell and the readout owns it.
    if (it.status === "active") return stopping ? "stopping…" : "connecting…";
    if (it.status === "result") return resultText(it);
    return "queued";
  };

  return (
    <>
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
            display: "flex",
            alignItems: "baseline",
            justifyContent: "space-between",
            margin: "11px 0 8px",
          }}
        >
          <span style={{ fontFamily: t.mono, fontSize: 12, color: t.ink2 }}>
            {done
              ? stopped
                ? "stopped"
                : "done"
              : `Step ${String(stepNo)} of ${String(total)}`}
          </span>
          <span style={{ fontFamily: t.mono, fontSize: 10.5, color: t.faint }}>
            {done ? "" : "saves automatically"}
          </span>
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
          const active = it.status === "active";
          const result = it.status === "result";
          // A non-null live value during an active item IS the "measuring" signal — events
          // only flow while a capture runs. The readout then owns the right status cell.
          // Bind the number (not a boolean) so TS narrows `live` to `number` at the readout.
          const live: number | null = active ? liveLufs : null;
          const statusColor = active
            ? t.sevWarn
            : result
              ? resultColor(it)
              : t.faint;
          return (
            <RunRow
              key={it.key}
              active={active}
              dim={it.status === "queued"}
              statusWidth={150}
              name={it.label}
              tag={it.tag ?? undefined}
              tagColor={it.isBase ? t.faint : t.accentDeep}
              instrument={it.instId ? instrumentName(it.instId) : undefined}
              icon={
                <>
                  {active && (
                    <Spinner size={14} stroke={t.sevWarn} strokeWidth={1.8} />
                  )}
                  {it.status === "queued" && <Dot color={t.faint} />}
                  {result &&
                    (it.outcome === "clamped" ? (
                      <Icon
                        name="warn-tri"
                        size={14}
                        stroke={t.sevWarn}
                        strokeWidth={1.7}
                      />
                    ) : it.outcome === "offbranch" ? (
                      <Icon
                        name="x"
                        size={14}
                        stroke={t.warn}
                        strokeWidth={2}
                      />
                    ) : it.outcome === "skipped" ? (
                      <Icon
                        name="x"
                        size={13}
                        stroke={t.mutedInk}
                        strokeWidth={2}
                      />
                    ) : (
                      <Icon
                        name="check"
                        size={15}
                        stroke={t.good}
                        strokeWidth={2}
                      />
                    ))}
                </>
              }
              status={
                <span style={{ color: statusColor }}>
                  {live !== null ? "" : rowStatus(it)}
                </span>
              }
            >
              {live !== null && (
                <div
                  style={{
                    display: "flex",
                    alignItems: "flex-end",
                    gap: 14,
                    marginTop: 10,
                    paddingLeft: 30,
                    paddingRight: 2,
                  }}
                >
                  <LiveVU values={liveTrace} />
                  <LiveReadout
                    value={live}
                    format={fmtLufs}
                    unit="LUFS"
                    caption="leveling…"
                  />
                </div>
              )}
            </RunRow>
          );
        })}
      </div>

      {confirm ? (
        <ConfirmBar
          message="Stop leveling? Progress so far stays saved."
          onCancel={() => {
            setConfirm(false);
          }}
          onConfirm={() => {
            setConfirm(false);
            onCancel();
          }}
        />
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
            ) : stopping ? (
              // Stop already requested — show the wind-down state, not a second Cancel.
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
                small
                onClick={() => {
                  setConfirm(true);
                }}
                style={{ height: 32, padding: "0 15px" }}
              >
                Cancel
              </Button>
            )
          }
        />
      )}
    </>
  );
}

export default RunBody;
