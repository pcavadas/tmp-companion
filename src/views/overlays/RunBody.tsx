// src/views/overlays/RunBody.tsx — wizard step 3, "Level" (running).
//
// Presentational: the useLevelingFlow hook drives the sequence (one chosen scene at a
// time, loading each on the unit, measuring, adjusting, saving) and updates the items'
// live status/outcome here.
//
// The list is a five-column TABLE (glyph · Sound · Instrument · Target · Result) sharing
// one grid template with its header row, because the old single concatenated
// "<preset> · <scene>" label ellipsised away exactly the part that identifies the sound
// and never stated the target. The scene/footswitch name owns the Sound column; its
// preset is the mono sub-line under it.
//
// The live capture meter lives in the step HEADER, not in the active row, and is gated by
// OPACITY rather than mounting — so no row ever grows and the list never shifts mid-run.
// Per-step Result: queued · active ("connecting…", then "leveling · <lufs>") · result
// (done · −24.0 / clamped · −25.8 / not on USB 1/2 / skipped · read failed).
//
// Completion: when the run finishes on its OWN it auto-advances to Summary after 650ms,
// showing a static "✓ done" marker in the footer (no flashing Continue button). A
// Continue button appears only when the user manually STOPPED the run. Cancel opens an
// inline confirm that replaces the footer.

import { useState } from "react";

import { useStyles, useTheme } from "../../theme/ThemeContext";
import { Button } from "../../ui/primitives";
import { Icon } from "../../ui/Icon";
import { Spinner } from "../../ui/Spinner";
import { Dot } from "../../ui/Dot";
import { ProgressBar } from "../../ui/ProgressBar";
import { LiveVU } from "../../ui/LiveVU";
import { LiveReadout } from "../../ui/LiveReadout";
import { ConfirmBar } from "../../ui/ConfirmBar";
import { RunRow, RUNROW_GLYPH_W } from "../../ui/RunRow";
import { WizardFooter, WizTitle } from "./WizardShell";
import { fmtLufs } from "../../lib/format";
import { useAutoAdvance } from "../../lib/useAutoAdvance";
import {
  offbranchStatus,
  presetLine,
  resolvedTargetLufs,
  type RunItem,
} from "../level/leveling";

/** The run table's shared grid: glyph · Sound (flexible) · Instrument · Target · Result.
 *  The header row and every RunRow lay out on THIS template, so the columns can't drift. */
const LV_COLS = `${String(RUNROW_GLYPH_W)}px minmax(0, 1fr) 124px 112px 158px`;
/** Live-meter frame. Fixed because LiveVU is `flex: 1` and the caption is variable-length —
 *  an intrinsic width would squeeze the progress bar as the active sound's name changes. */
const LV_METER_W = 292;

export interface RunBodyProps {
  items: RunItem[];
  currentIndex: number;
  total: number;
  done: boolean;
  stopped: boolean;
  /** Stop requested; the in-flight item is winding down (no row is truly idle yet). */
  stopping: boolean;
  /** Advisory live measured loudness for the header's capture meter (null = nothing
   *  streaming). Reference-level, NOT the final value — the result row is the confirm. */
  liveLufs: number | null;
  /** Rolling per-hop momentary levels (dB, newest last) driving the live VU bars. */
  liveTrace: number[];
  /** Resolve an instrument profile id to its display name (the Instrument column). */
  instrumentName: (id: string) => string;
  /** Resolve a target name to its LUFS (the Target column). */
  targetLufsByName: (name: string | null) => number;
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
  targetLufsByName,
  onCancel,
  onComplete,
}: RunBodyProps) {
  const { t } = useTheme();
  const s = useStyles();
  const [confirm, setConfirm] = useState(false);

  // Natural completion auto-advances; a stopped run waits for Continue.
  useAutoAdvance(done, stopped, onComplete);

  const stepNo = Math.min(currentIndex + 1, total);
  // currentIndex reaches `total` on a natural finish (→ 100%) and stays partial on a
  // stop, so the bare ratio covers every case — no done/stopped branching needed.
  const pct = total > 0 ? (currentIndex / total) * 100 : 0;
  const presetN = new Set(items.map((x) => x.slot)).size;
  const activeItem = items.find((x) => x.status === "active");
  const colHead = s.kickerWide(t.faint);

  const resultText = (it: RunItem): string => {
    if (it.outcome === "clamped") return `clamped · ${fmtLufs(it.value)}`;
    // A clamped sound is at its limit; this one just missed — hence a different word and
    // no "clamped", which would tell the user re-running is pointless when it isn't.
    if (it.outcome === "unconverged")
      return `off target · ${fmtLufs(it.value)}`;
    if (it.outcome === "offbranch") return offbranchStatus(it.silenceHint);
    if (it.outcome === "skipped") return "skipped · read failed";
    return `done · ${fmtLufs(it.value)}`;
  };
  const resultColor = (it: RunItem): string =>
    it.outcome === "clamped" || it.outcome === "unconverged"
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
  // The row states what it is ACTUALLY aiming at — `resolvedTargetLufs` is the same
  // resolution the run loop dispatches on, so the cell can't drift from the run.
  const targetText = (it: RunItem): string =>
    `${it.targetName} · ${fmtLufs(resolvedTargetLufs(it, targetLufsByName))}`;
  const rowStatus = (it: RunItem): string => {
    // Active-but-not-yet-streaming = loading the preset + engaging re-amp (no LUFS events
    // yet), so "connecting…" is truer than "leveling…" for that pre-capture window — unless
    // the backend handed a specific reason (e.g. the freshness barrier waiting out the TMP's
    // lazy `saveCurrentPreset` commit window), which takes priority over the generic default.
    if (it.status === "active")
      return stopping
        ? "stopping…"
        : liveLufs !== null
          ? `leveling · ${fmtLufs(liveLufs)}`
          : (it.activeMessage ?? "connecting…");
    if (it.status === "result") return resultText(it);
    // A stopped run's tail would otherwise read as still-pending forever.
    return stopped ? "—" : "queued";
  };

  return (
    <>
      <div
        style={{
          flexShrink: 0,
          display: "flex",
          alignItems: "flex-end",
          gap: t.space10,
          padding: `${String(t.space8)}px ${String(t.space10)}px ${String(t.space7)}px`,
          borderBottom: `0.5px solid ${t.hairline}`,
        }}
      >
        <div style={{ flex: 1, minWidth: 0 }}>
          <WizTitle>{headerTitle()}</WizTitle>
          <div
            style={{
              display: "flex",
              alignItems: "baseline",
              gap: t.space6,
              margin: `${String(t.space5)}px 0 ${String(t.space4)}px`,
            }}
          >
            <span
              style={{
                fontFamily: t.mono,
                fontSize: t.fsUi,
                color: t.ink2,
                whiteSpace: "nowrap",
                flexShrink: 0,
              }}
            >
              {done
                ? stopped
                  ? "stopped"
                  : "done"
                : `Step ${String(stepNo)} of ${String(total)}`}
            </span>
            <span
              style={{
                fontFamily: t.mono,
                fontSize: t.fsMeta,
                color: t.faint,
                whiteSpace: "nowrap",
                overflow: "hidden",
                textOverflow: "ellipsis",
              }}
            >
              {done
                ? ""
                : `${String(presetN)} preset${presetN === 1 ? "" : "s"} · ${String(total)} sound${total === 1 ? "" : "s"} · saves automatically`}
            </span>
          </div>
          <ProgressBar percent={pct} />
        </div>
        {/* Opacity-gated, never unmounted: the header must not change height between
            items (that is the whole reason the meter moved out of the active row). */}
        <div
          aria-hidden={liveLufs === null}
          style={{
            width: LV_METER_W,
            flexShrink: 0,
            overflow: "hidden",
            display: "flex",
            alignItems: "flex-end",
            gap: t.space7,
            opacity: liveLufs === null ? 0 : 1,
            transition: "opacity 0.3s ease",
          }}
        >
          <LiveVU values={liveTrace} />
          <LiveReadout
            value={liveLufs ?? 0}
            format={fmtLufs}
            unit="LUFS"
            caption={
              activeItem
                ? `measuring ${activeItem.tag ?? activeItem.sceneName}`
                : "measuring…"
            }
          />
        </div>
      </div>

      <div
        style={{
          flex: 1,
          minHeight: 0,
          overflowY: "auto",
          overflowX: "hidden",
          padding: `0 ${String(t.space7)}px ${String(t.space3)}px`,
        }}
      >
        {/* Sticky INSIDE the scroller: the header then shares the rows' content width, so
            no scrollbar-gutter maths is needed to keep the columns aligned. */}
        <div
          style={{
            position: "sticky",
            top: 0,
            zIndex: 1,
            background: t.bg,
            display: "grid",
            gridTemplateColumns: LV_COLS,
            gap: t.space7,
            padding: `${String(t.space6)}px ${String(t.space5)}px ${String(t.space4)}px`,
            borderBottom: `0.5px solid ${t.hairline}`,
          }}
        >
          <span />
          <span style={colHead}>Sound</span>
          <span style={colHead}>Instrument</span>
          <span style={colHead}>Target</span>
          <span style={{ ...colHead, textAlign: "right" }}>Result</span>
        </div>
        <div style={{ paddingTop: t.space4 }}>
          {items.map((it) => {
            const active = it.status === "active";
            const result = it.status === "result";
            const statusColor = active
              ? t.sevWarn
              : result
                ? resultColor(it)
                : t.faint;
            return (
              <RunRow
                key={it.key}
                columns={LV_COLS}
                active={active}
                dim={it.status === "queued"}
                name={it.sceneName}
                subLabel={presetLine(it)}
                tag={it.tag ?? undefined}
                tagColor={it.isBase ? t.faint : t.accentDeep}
                instrument={it.instId ? instrumentName(it.instId) : undefined}
                target={targetText(it)}
                icon={
                  <>
                    {active && (
                      <Spinner size={14} stroke={t.sevWarn} strokeWidth={1.8} />
                    )}
                    {/* A stopped run's untouched tail reads as never-ran, not pending. */}
                    {it.status === "queued" && (
                      <Dot color={t.faint} hollow={stopped} />
                    )}
                    {result &&
                      (it.outcome === "clamped" ? (
                        <Icon
                          name="warn-tri"
                          size={14}
                          stroke={t.sevWarn}
                          strokeWidth={1.7}
                        />
                      ) : it.outcome === "unconverged" ? (
                        // Shape, not colour alone, separates "ran out of knob" (warning
                        // triangle) from "ran out of tries" (re-run).
                        <Icon
                          name="refresh"
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
                  <span style={{ color: statusColor }}>{rowStatus(it)}</span>
                }
              />
            );
          })}
        </div>
      </div>

      {confirm && !done ? (
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
                style={{ height: 32, padding: `0 ${String(t.space8)}px` }}
              >
                Continue
              </Button>
            ) : done ? (
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
                style={{ height: 32, padding: `0 ${String(t.space8)}px` }}
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
