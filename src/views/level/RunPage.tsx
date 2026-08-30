// src/views/level/RunPage.tsx — the leveling wizard's "Level" (run) stage (design
// handoff 1a: full window, one row per preset). Replaces `overlays/RunBody.tsx`.
//
// Presentational: `useLevelingFlow` drives the sequence. The list groups by PRESET
// (`PresetGroupRow`) — the preset currently being worked on opens itself
// (`useGroupOpen`), following the run; the user can override any row on top of that.
// The live number renders ONCE, in the header (never in a static preset row) — sound
// rows show it only while they're the active one, so no row ever changes height or
// shifts mid-run.

import { useMemo, useState } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Button } from "../../ui/primitives";
import { ConfirmBar } from "../../ui/ConfirmBar";
import { Icon } from "../../ui/Icon";
import { Spinner } from "../../ui/Spinner";
import { Dot } from "../../ui/Dot";
import { ProgressBar } from "../../ui/ProgressBar";
import { LiveVU } from "../../ui/LiveVU";
import { LiveReadout } from "../../ui/LiveReadout";
import { LevelPage } from "./LevelPage";
import { PresetGroupList, PresetGroupRow } from "./PresetGroupRow";
import { useGroupOpen } from "./useGroupOpen";
import { fmtLufs } from "../../lib/format";
import { useAutoAdvance } from "../../lib/useAutoAdvance";
import {
  groupItemsBySlot,
  offbranchStatus,
  resolvedTargetLufs,
  type RunItem,
} from "./leveling";

export interface RunPageProps {
  items: RunItem[];
  currentIndex: number;
  total: number;
  done: boolean;
  stopped: boolean;
  stopping: boolean;
  liveLufs: number | null;
  liveTrace: number[];
  tailMessage?: string | null;
  instrumentName: (id: string) => string;
  targetLufsByName: (name: string | null) => number;
  onCancel: () => void;
  onComplete: () => void;
}

export function RunPage({
  items,
  currentIndex,
  total,
  done,
  stopped,
  stopping,
  liveLufs,
  liveTrace,
  tailMessage = null,
  instrumentName,
  targetLufsByName,
  onCancel,
  onComplete,
}: RunPageProps) {
  const { t } = useTheme();
  const [confirm, setConfirm] = useState(false);

  useAutoAdvance(done, stopped, onComplete);

  const pct = total > 0 ? (currentIndex / total) * 100 : 0;
  const presetN = new Set(items.map((x) => x.slot)).size;
  const activeItem = items.find((x) => x.status === "active");
  const groups = useMemo(() => groupItemsBySlot(items), [items]);
  const [isOpen, toggle] = useGroupOpen(activeItem ? [activeItem.slot] : []);

  const resultText = (it: RunItem): string => {
    if (it.outcome === "clamped") return `clamped · ${fmtLufs(it.value)}`;
    if (it.outcome === "unconverged")
      return `off target · ${fmtLufs(it.value)}`;
    if (it.outcome === "offbranch") return offbranchStatus(it.silenceHint);
    if (it.outcome === "skipped") return "skipped · read failed";
    return `${fmtLufs(it.value)} LUFS`;
  };
  const resultColor = (it: RunItem): string =>
    it.outcome === "clamped" || it.outcome === "unconverged"
      ? t.sevWarn
      : it.outcome === "offbranch"
        ? t.warn
        : it.outcome === "skipped"
          ? t.mutedInk
          : t.good;
  const rowStatus = (it: RunItem): string => {
    if (it.status === "active")
      return stopping
        ? "stopping…"
        : liveLufs !== null
          ? `${it.activeMessage ?? "leveling"} · ${fmtLufs(liveLufs)}`
          : (it.activeMessage ?? "connecting…");
    if (it.status === "result") return resultText(it);
    return stopped ? "—" : "waiting";
  };
  const targetText = (it: RunItem): string =>
    `${it.targetName} · ${fmtLufs(resolvedTargetLufs(it, targetLufsByName))}`;

  const headerTitle = stopping
    ? "Stopping…"
    : stopped
      ? "Leveling stopped"
      : done
        ? "Leveling complete"
        : activeItem
          ? `Listening to sound ${String(currentIndex + 1)} of ${String(total)}`
          : "Saving the last one…";
  const headerSub = activeItem
    ? `${activeItem.presetName}, ${activeItem.sceneName}.`
    : "";

  return (
    <LevelPage
      step={1}
      title={headerTitle}
      sub={headerSub}
      right={
        <div style={{ flexShrink: 0, textAlign: "right", minWidth: 132 }}>
          <div
            style={{ display: "flex", alignItems: "flex-end", gap: t.space5 }}
          >
            <LiveVU values={liveTrace} />
            <LiveReadout
              value={liveLufs}
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
      }
      footerLeft={
        <Button
          variant="ghost"
          small
          disabled={done}
          onClick={() => {
            setConfirm(true);
          }}
          style={{ height: 32, padding: `0 ${String(t.space8)}px` }}
        >
          Stop
        </Button>
      }
      footerRight={
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
        ) : (
          <span style={{ fontFamily: t.mono, fontSize: 11, color: t.mutedInk }}>
            Each sound is saved as soon as it matches
          </span>
        )
      }
      footerOverride={
        confirm ? (
          <ConfirmBar
            message="Stop leveling? Everything matched so far stays saved."
            cancelLabel="Keep going"
            onCancel={() => {
              setConfirm(false);
            }}
            onConfirm={() => {
              setConfirm(false);
              onCancel();
            }}
          />
        ) : undefined
      }
    >
      <div style={{ flexShrink: 0 }}>
        <ProgressBar percent={pct} height={4} />
        {!done && (
          <div
            style={{
              marginTop: t.space3,
              fontFamily: t.mono,
              fontSize: 10.5,
              color: t.faint,
            }}
          >
            {tailMessage ??
              `${String(presetN)} preset${presetN === 1 ? "" : "s"} · ${String(total)} sound${total === 1 ? "" : "s"} · saves automatically`}
          </div>
        )}
      </div>

      <PresetGroupList
        label={`${String(groups.length)} preset${groups.length === 1 ? "" : "s"} · ${String(currentIndex)} of ${String(total)} matched`}
      >
        {groups.map((g) => {
          const states = g.items.map((it) => it.status);
          const allDone = states.every((s) => s === "result");
          const running = states.some((s) => s === "active");
          return (
            <PresetGroupRow
              key={g.slot}
              slot={g.slot}
              name={g.name}
              open={isOpen(g.slot)}
              onToggle={() => {
                toggle(g.slot);
              }}
              glyph={
                allDone ? (
                  <Icon
                    name="check"
                    size={14}
                    stroke={t.good}
                    strokeWidth={2}
                  />
                ) : running ? (
                  <Dot color={t.accent} />
                ) : (
                  <Dot color={t.faint} hollow />
                )
              }
              tag={
                <span
                  style={{
                    fontFamily: t.mono,
                    fontSize: 9.5,
                    letterSpacing: "0.07em",
                    color: allDone ? t.good : running ? t.accentDeep : t.faint,
                  }}
                >
                  {allDone
                    ? "done"
                    : running
                      ? "listening"
                      : `${String(g.items.length)} sound${g.items.length === 1 ? "" : "s"}`}
                </span>
              }
              note=""
              value={
                allDone
                  ? `${fmtLufs(resolvedTargetLufs(g.items[0], targetLufsByName))} LUFS`
                  : running
                    ? ""
                    : "waiting"
              }
              valueColor={allDone ? t.good : t.faint}
            >
              {g.items.map((it) => {
                const active = it.status === "active";
                const result = it.status === "result";
                return (
                  <div
                    key={it.key}
                    style={{
                      display: "flex",
                      alignItems: "center",
                      gap: t.space6,
                      minHeight: 30,
                    }}
                  >
                    <span
                      style={{
                        width: 14,
                        flexShrink: 0,
                        display: "inline-flex",
                        justifyContent: "center",
                      }}
                    >
                      {active && (
                        <Spinner
                          size={14}
                          stroke={t.sevWarn}
                          strokeWidth={1.8}
                        />
                      )}
                      {it.status === "queued" && (
                        <Dot color={t.faint} hollow={stopped} />
                      )}
                      {result &&
                        (it.outcome === "clamped" ? (
                          <Icon
                            name="warn-tri"
                            size={13}
                            stroke={t.sevWarn}
                            strokeWidth={1.7}
                          />
                        ) : it.outcome === "unconverged" ? (
                          <Icon
                            name="refresh"
                            size={13}
                            stroke={t.sevWarn}
                            strokeWidth={1.7}
                          />
                        ) : it.outcome === "offbranch" ? (
                          <Icon
                            name="x"
                            size={13}
                            stroke={t.warn}
                            strokeWidth={2}
                          />
                        ) : it.outcome === "skipped" ? (
                          <Icon
                            name="x"
                            size={12}
                            stroke={t.mutedInk}
                            strokeWidth={2}
                          />
                        ) : (
                          <Icon
                            name="check"
                            size={13}
                            stroke={t.good}
                            strokeWidth={2}
                          />
                        ))}
                    </span>
                    <span
                      style={{
                        fontFamily: t.sans,
                        fontSize: 13,
                        color: it.status === "queued" ? t.faint : t.ink2,
                        width: 138,
                        flexShrink: 0,
                        whiteSpace: "nowrap",
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                      }}
                    >
                      {it.sceneName}
                    </span>
                    <span
                      style={{
                        flex: 1,
                        minWidth: 0,
                        fontFamily: t.mono,
                        fontSize: 10.5,
                        color: t.faint,
                        whiteSpace: "nowrap",
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                      }}
                    >
                      {it.instId ? `${instrumentName(it.instId)} · ` : ""}
                      {targetText(it)}
                    </span>
                    <span
                      style={{
                        width: 120,
                        textAlign: "right",
                        flexShrink: 0,
                        fontFamily: t.sans,
                        fontSize: 12,
                        color: active
                          ? t.sevWarn
                          : result
                            ? resultColor(it)
                            : t.faint,
                      }}
                    >
                      {rowStatus(it)}
                    </span>
                  </div>
                );
              })}
            </PresetGroupRow>
          );
        })}
      </PresetGroupList>
    </LevelPage>
  );
}

export default RunPage;
