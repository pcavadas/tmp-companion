// src/views/copy/SaveOverlay.tsx — saving to your Tone Master Pro (look B).
//
// Driven by the REAL `copy_apply` run (the channel streams one result per preset), NOT a
// synthetic timer: `results` fills as each preset commits, `activeSlot` is the one being
// written. Place blocks stays under the scrim; the dialog never closes on a backdrop click
// and has no button while it saves. It shows a progress bar ("Saving 2 presets"), one row
// per preset being saved (Waiting, Saving, then Saved / No change / Couldn't save), a line
// naming the ticked presets that are unchanged and so not saved, and "Keep your Tone
// Master Pro connected."
//
// Done, the title says the outcome, read from how many presets actually saved: all
// ("Saved to your Tone Master Pro" — the backup is how to undo it), some ("Some presets
// weren't saved" — the others keep their changes), or none ("Nothing was saved"). One
// Done closes it. A row's error detail and a rejected run's text go to the log only.

import { useTheme } from "../../theme/ThemeContext";
import {
  Dialog,
  DialogBody,
  DialogFooter,
  DialogHeader,
} from "../../ui/Dialog";
import { Dot } from "../../ui/Dot";
import { Icon } from "../../ui/Icon";
import { Group, Row } from "../../ui/List";
import { ProgressBar } from "../../ui/ProgressBar";
import { Spinner } from "../../ui/Spinner";
import { Button } from "../../ui/primitives";
import type { CopyApplyItem } from "../../lib/types";
import {
  COPY_TEXT,
  savingPresets,
  someSavedBody,
  unchangedLine,
} from "./copyText";

export interface SaveOverlayProps {
  slots: number[];
  nameOf: (slot: number) => string;
  /** Per-slot outcome as it streams in. */
  results: Map<number, CopyApplyItem>;
  /** The slot currently being written (null once done). */
  activeSlot: number | null;
  /** The run settled (resolved or rejected). */
  done: boolean;
  /** The ticked presets with nothing to save (not in `slots`), by name. */
  unchanged: readonly string[];
  onDone: () => void;
}

type RowStatus = "waiting" | "saving" | "saved" | "nochange" | "failed";

export function SaveOverlay({
  slots,
  nameOf,
  results,
  activeSlot,
  done,
  unchanged,
  onDone,
}: SaveOverlayProps) {
  const { t } = useTheme();
  const total = slots.length;
  const pct = total === 0 ? 100 : Math.round((results.size / total) * 100);
  // A target saved when it has a non-error result; an error row, or no row at all (a
  // rejected or stopped run), means it did not.
  const saved = slots.filter((s) => {
    const r = results.get(s);
    return r != null && r.outcome !== "error";
  }).length;
  const unsaved = total - saved;
  const outcome: "all" | "partial" | "none" =
    unsaved === 0 ? "all" : saved === 0 ? "none" : "partial";
  const title = !done
    ? COPY_TEXT.saving
    : outcome === "all"
      ? COPY_TEXT.allSaved
      : outcome === "partial"
        ? COPY_TEXT.someSaved
        : COPY_TEXT.noneSaved;
  const body = !done
    ? null
    : outcome === "all"
      ? COPY_TEXT.allSavedBody
      : outcome === "partial"
        ? someSavedBody(saved, unsaved)
        : COPY_TEXT.noneSavedBody;
  const unchangedText = unchangedLine(unchanged);

  const statusOf = (s: number): RowStatus => {
    const r = results.get(s);
    if (r) {
      return r.outcome === "error"
        ? "failed"
        : r.outcome === "skipped"
          ? "nochange"
          : "saved";
    }
    return !done && s === activeSlot ? "saving" : "waiting";
  };

  const note = {
    fontSize: t.fontCaption,
    lineHeight: `${String(t.lineCaption)}px`,
    color: t.textSecondary,
    margin: 0,
  } as const;

  return (
    <Dialog size="progress" zIndex={60} label={title}>
      <DialogHeader>
        <h2
          style={{
            margin: 0,
            fontSize: t.fontLarge,
            lineHeight: `${String(t.lineLarge)}px`,
            fontWeight: t.weightSemibold,
            color: t.text,
          }}
        >
          {title}
        </h2>
      </DialogHeader>
      <DialogBody>
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            gap: t.sp8 + t.sp2,
            paddingBottom: done ? 0 : t.sp16,
          }}
        >
          {body != null && <p style={{ margin: 0 }}>{body}</p>}
          {!done && (
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: t.sp8 + t.sp2,
              }}
            >
              <div
                role="progressbar"
                aria-label={savingPresets(total)}
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={pct}
                style={{ flex: 1, minWidth: 0 }}
              >
                <ProgressBar percent={pct} />
              </div>
              <span
                style={{
                  ...note,
                  whiteSpace: "nowrap",
                  fontVariantNumeric: "tabular-nums",
                }}
              >
                {savingPresets(total)}
              </span>
            </div>
          )}
          <Group>
            {slots.map((s) => (
              <SaveRow key={s} name={nameOf(s)} status={statusOf(s)} />
            ))}
          </Group>
          {unchangedText != null && <p style={note}>{unchangedText}</p>}
          {!done && <p style={note}>{COPY_TEXT.keepConnected}</p>}
        </div>
      </DialogBody>
      {done && (
        <DialogFooter>
          <Button variant="primary" onClick={onDone}>
            {COPY_TEXT.done}
          </Button>
        </DialogFooter>
      )}
    </Dialog>
  );
}

interface SaveRowProps {
  name: string;
  status: RowStatus;
}

const ROW_WORD: Record<RowStatus, string> = {
  waiting: COPY_TEXT.rowWaiting,
  saving: COPY_TEXT.rowSaving,
  saved: COPY_TEXT.rowSaved,
  nochange: COPY_TEXT.rowNoChange,
  failed: COPY_TEXT.rowFailed,
};

// One preset being saved: its mark, its name and one word for where it is.
function SaveRow({ name, status }: SaveRowProps) {
  const { t } = useTheme();
  return (
    <Row size="compact" data-save-row={name}>
      <span
        style={{
          width: 14,
          flexShrink: 0,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        {status === "saving" && <Spinner size={14} stroke={t.textSecondary} />}
        {status === "waiting" && <Dot hollow tone="neutral" />}
        {(status === "saved" || status === "nochange") && (
          <Icon name="check" size={14} stroke={t.good} strokeWidth={2.2} />
        )}
        {status === "failed" && (
          <Icon name="warn-tri" size={14} stroke={t.caution} strokeWidth={2} />
        )}
      </span>
      <span
        style={{
          flex: 1,
          minWidth: 0,
          fontWeight: t.weightMedium,
          whiteSpace: "nowrap",
          overflow: "hidden",
          textOverflow: "ellipsis",
        }}
      >
        {name}
      </span>
      <span
        style={{
          fontSize: t.fontCaption,
          color: status === "failed" ? t.text : t.textSecondary,
          whiteSpace: "nowrap",
        }}
      >
        {ROW_WORD[status]}
      </span>
    </Row>
  );
}
