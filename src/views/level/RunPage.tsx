// src/views/level/RunPage.tsx — the leveling run, a page under the toolbar (look B).
//
// Top to bottom: the title ("Leveling 6 sounds" — constant; a Try again counts only the
// sounds it tries again) with the flow's Steps; the now card: on the left the now line
// ("Listening to <sound> in <preset>") over the LoudnessMeter (the loudness now against
// the sound's target, no numbers), on the right the progress bar and "3 of 6 done"; one
// Group, a header per preset (name and slot) and a row per sound (status mark, name,
// target, state word); and the Footer with Stop on the left.
//
// Presentational: `levelRunStore` drives the run; the live meter is read from its own
// store (`useLiveMeter`) right here, so its ~5 Hz ticks re-render this page alone. The
// list scrolls under the now card and keeps the sound in progress in view. Stop asks first
// in a dialog that only its own buttons close (a stray click must not stop a run, and
// must not dismiss the question either); the run keeps going while it is open. After a
// Stop the page says it is stopping after the sound in progress, and the store lands on
// the Summary once that sound has wound down; a natural finish shows the last row for a
// beat (`useAutoAdvance`) before the Summary. No saving line here: the backup tick was
// the one safety line, and the Stop dialog says what stays saved.

import { useEffect, useMemo, useRef, useState } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Footer } from "../../ui/Footer";
import { Group, Row } from "../../ui/List";
import { LoudnessMeter } from "../../ui/LoudnessMeter";
import { ProgressBar } from "../../ui/ProgressBar";
import { SlotLabel } from "../../ui/SlotLabel";
import { Button, Modal } from "../../ui/primitives";
import { useAutoAdvance } from "../../lib/useAutoAdvance";
import { FlowPage } from "./FlowPage";
import { StatusMark } from "./StatusMark";
import { useLiveMeter } from "./liveMeterStore";
import { isRunOver, type RunPhase } from "./levelRunStore";
import {
  groupItemsBySlot,
  setupRowHookKey,
  type RunItem,
  type TargetLufsOf,
} from "./leveling";
import { footswitchNo, nowLine, runRowState } from "./runModel";

export interface RunPageProps {
  items: RunItem[];
  currentIndex: number;
  total: number;
  phase: RunPhase;
  tailMessage?: string | null;
  targetLufsByName: TargetLufsOf;
  /** The Stop dialog's "Stop": stop after the sound in progress. */
  onStop: () => void;
  /** After a natural finish: on to the Summary. */
  onComplete: () => void;
}

export function RunPage({
  items,
  currentIndex,
  total,
  phase,
  tailMessage = null,
  targetLufsByName,
  onStop,
  onComplete,
}: RunPageProps) {
  const { t } = useTheme();
  const [asking, setAsking] = useState(false);
  const { lufs } = useLiveMeter();
  const over = isRunOver(phase);
  const stopping = phase === "stopping";

  useAutoAdvance(over, phase === "stopped", onComplete);

  const groups = useMemo(() => groupItemsBySlot(items), [items]);
  const done = items.filter((x) => x.status === "result").length;
  const now = nowLine(items, currentIndex, phase, tailMessage);
  const focus = now.item ?? (items.length > 0 ? items[0] : null);
  // The loudness a row is leveled to: the one its batch resolved, else its target's now.
  // A row whose target matches none has no meter (the run refuses it).
  const lufsOf = (it: RunItem): number | null =>
    it.targetLufs ?? targetLufsByName(it.targetName);
  const focusLufs = focus ? lufsOf(focus) : null;

  // Keep the sound in progress in view as the run moves down the list.
  const listRef = useRef<HTMLDivElement>(null);
  const activeKey = items.find((x) => x.status === "active")?.key ?? null;
  useEffect(() => {
    if (activeKey == null) return;
    listRef.current
      ?.querySelector(`[data-run-row-key="${activeKey}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }, [activeKey]);

  return (
    <FlowPage
      title={`Leveling ${String(total)} ${total === 1 ? "sound" : "sounds"}`}
      step={1}
      fill
      footer={
        <Footer
          left={
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
          }
        />
      }
    >
      <div
        style={{
          flexShrink: 0,
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: t.sp24,
          padding: `${String(t.sp12 + t.sp2)}px ${String(t.sp16)}px`,
          background: t.surface,
          border: `1px solid ${t.border}`,
          borderRadius: t.radiusGroup,
        }}
      >
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            gap: t.sp8 + t.sp2,
            minWidth: 0,
            flex: 1,
          }}
        >
          <span
            aria-live="polite"
            style={{
              fontSize: t.fontLarge,
              lineHeight: `${String(t.lineLarge)}px`,
              fontWeight: t.weightSemibold,
              whiteSpace: "nowrap",
              overflow: "hidden",
              textOverflow: "ellipsis",
            }}
          >
            {now.text}
          </span>
          {focus && focusLufs != null && (
            <LoudnessMeter
              lufs={now.listening ? lufs : null}
              target={focusLufs}
              targetName={focus.targetName}
              soundName={focus.sceneName}
            />
          )}
        </div>
        <div
          style={{
            flexShrink: 0,
            display: "flex",
            flexDirection: "column",
            alignItems: "flex-end",
            gap: t.sp6,
          }}
        >
          <div
            role="progressbar"
            aria-label="Sounds done"
            aria-valuemin={0}
            aria-valuemax={total}
            aria-valuenow={done}
            style={{ width: 140 }}
          >
            <ProgressBar percent={total > 0 ? (done / total) * 100 : 0} />
          </div>
          <span
            style={{
              fontSize: t.fontCaption,
              lineHeight: `${String(t.lineCaption)}px`,
              color: t.textSecondary,
              whiteSpace: "nowrap",
              fontVariantNumeric: "tabular-nums",
            }}
          >
            {`${String(done)} of ${String(total)} done`}
          </span>
        </div>
      </div>

      <div
        ref={listRef}
        style={{ flex: "0 1 auto", minHeight: 0, overflowY: "auto" }}
      >
        <Group label="Sounds being leveled">
          {groups.map((g) => (
            <div key={g.slot}>
              <Row header size="compact">
                <span style={{ fontWeight: t.weightSemibold }}>{g.name}</span>
                <SlotLabel index={g.slot} />
              </Row>
              {g.items.map((it) => (
                <RunRow
                  key={it.key}
                  item={it}
                  stopping={stopping}
                  targetLufs={lufsOf(it)}
                />
              ))}
            </div>
          ))}
        </Group>
      </div>

      <Modal
        open={asking && !over}
        label="Stop leveling?"
        headline="Stop leveling?"
        body="Everything matched so far stays saved."
        cancelLabel="Keep leveling"
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

interface RunRowProps {
  item: RunItem;
  stopping: boolean;
  targetLufs: number | null;
}

/** One sound of the run: mark, name (a footswitch says which one), target, state word. */
function RunRow({ item, stopping, targetLufs }: RunRowProps) {
  const { t } = useTheme();
  const state = runRowState(item, stopping, targetLufs);
  const fs = footswitchNo(item);
  const secondary = {
    fontSize: t.fontCaption,
    color: t.textSecondary,
    whiteSpace: "nowrap",
  } as const;
  return (
    <Row
      size="compact"
      data-run-row={setupRowHookKey(item)}
      data-run-row-key={item.key}
    >
      <StatusMark kind={state.mark} />
      <span
        style={{
          flex: 1,
          minWidth: 0,
          display: "flex",
          alignItems: "baseline",
          gap: t.sp8,
          overflow: "hidden",
        }}
      >
        <span
          style={{
            whiteSpace: "nowrap",
            overflow: "hidden",
            textOverflow: "ellipsis",
          }}
        >
          {item.sceneName}
        </span>
        {fs != null && (
          <span style={secondary}>{`footswitch ${String(fs)}`}</span>
        )}
      </span>
      <span
        style={{
          ...secondary,
          width: 70,
          flexShrink: 0,
          overflow: "hidden",
          textOverflow: "ellipsis",
        }}
      >
        {item.targetName}
      </span>
      <span
        style={{
          width: 150,
          flexShrink: 0,
          textAlign: "right",
          fontSize: t.fontCaption,
          whiteSpace: "nowrap",
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
}
