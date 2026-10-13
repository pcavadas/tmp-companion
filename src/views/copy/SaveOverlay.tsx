// src/views/copy/SaveOverlay.tsx — the Save progress → done overlay.
//
// Driven by the REAL `copy_apply` run (the channel streams one result per preset), NOT a
// synthetic timer: `results` fills as each preset commits, `activeSlot` is the one being
// written. Progress phase lists every target slot with a status icon (spinner / queued
// dot / green check / warn for an error). The done phase has three outcomes, derived from
// how many targets actually saved: ALL saved → a green medallion + the count + a Pro
// Control backup-restore note (writes to the unit are not undoable from here — undo/redo
// is offline, pre-save only); SOME or NONE saved → a warn medallion, the counts, and a
// note that the unsaved presets keep their changes (Done returns to editing them). A
// single Done button either way.

import { useTheme } from "../../theme/ThemeContext";
import { Dialog, DialogBody, DialogFooter } from "../../ui/Dialog";
import { Icon } from "../../ui/Icon";
import { Button } from "../../ui/primitives";
import { ProgressBar } from "../../ui/ProgressBar";
import { Spinner } from "../../ui/Spinner";
import { Dot } from "../../ui/Dot";
import { slotLabel } from "../../lib/format";
import type { CopyApplyItem } from "../../lib/types";

export interface SaveOverlayProps {
  slots: number[];
  nameOf: (slot: number) => string;
  /** Per-slot outcome as it streams in. */
  results: Map<number, CopyApplyItem>;
  /** The slot currently being written (null once done). */
  activeSlot: number | null;
  /** Every target has a result. */
  done: boolean;
  /** Set when the whole `copy_apply` run REJECTED (device error / lost connection), as
   *  opposed to per-preset errors that stream into `results`. Shown in the done phase;
   *  the outcome itself is derived from `results` (a rejected run may have saved some
   *  presets before it failed). */
  failed?: string | null;
  onDone: () => void;
}

export function SaveOverlay({
  slots,
  nameOf,
  results,
  activeSlot,
  done,
  failed,
  onDone,
}: SaveOverlayProps) {
  const { t } = useTheme();
  const total = slots.length;
  const doneCount = results.size;
  const pct = total === 0 ? 100 : Math.round((doneCount / total) * 100);
  // A target saved when it has a non-error result; an error row, or no row at all (a
  // rejected or stopped run), means it did not.
  const saved = slots.filter((s) => {
    const r = results.get(s);
    return r != null && r.outcome !== "error";
  }).length;
  const unsaved = total - saved;
  const outcome: "all" | "partial" | "none" =
    unsaved === 0 ? "all" : saved === 0 ? "none" : "partial";
  const ok = outcome === "all";
  const title =
    outcome === "all"
      ? "Saved to the unit."
      : outcome === "partial"
        ? "Some presets weren’t saved."
        : "Save failed.";
  const plural = (n: number): string => (n === 1 ? "" : "s");

  return (
    <Dialog size="sm" zIndex={60} label="Saving to the unit">
      <DialogBody>
        <div style={{ paddingBottom: t.space7 }}>
          {done ? (
            <div style={{ textAlign: "center" }}>
              <span
                style={{
                  width: 48,
                  height: 48,
                  borderRadius: t.rPill,
                  display: "inline-flex",
                  alignItems: "center",
                  justifyContent: "center",
                  background: ok ? t.goodSoft : t.warnSoft,
                  border: `0.5px solid ${ok ? t.goodBorder : t.warnBorder}`,
                }}
              >
                <Icon
                  name={ok ? "check" : "warn-tri"}
                  size={24}
                  stroke={ok ? t.good : t.warn}
                  strokeWidth={2.2}
                />
              </span>
              <div
                style={{
                  fontFamily: t.serif,
                  fontSize: t.fsTitle,
                  color: t.ink,
                  marginTop: t.space6,
                }}
              >
                {title}
              </div>
              <div
                style={{
                  fontFamily: t.sans,
                  fontSize: t.fsBody2,
                  color: t.mutedInk,
                  marginTop: t.space3,
                }}
              >
                {outcome === "none" ? (
                  (failed ??
                  `${String(total)} preset${plural(total)} could not be saved.`)
                ) : (
                  <>
                    {String(saved)} preset{plural(saved)} updated
                    {unsaved > 0
                      ? ` · ${String(unsaved)} could not be saved`
                      : ""}
                    .{failed ? ` ${failed}` : ""}
                  </>
                )}
              </div>
              {/* Once written, the unit can't be rolled back from here — the only
                  recovery is restoring a Pro Control backup (undo/redo was offline,
                  pre-save only). Unsaved presets keep their staged changes. */}
              <div
                style={{
                  display: "flex",
                  alignItems: "flex-start",
                  gap: t.space5,
                  marginTop: t.space8,
                  padding: `${String(t.space5)}px ${String(t.space6)}px`,
                  borderRadius: t.rMd,
                  background: t.bgAlt,
                  textAlign: "left",
                }}
              >
                <span style={{ flexShrink: 0, marginTop: t.space1 }}>
                  <Icon name="refresh" size={15} stroke={t.faint} />
                </span>
                <span
                  style={{
                    fontFamily: t.sans,
                    fontSize: t.fsLabel,
                    lineHeight: 1.45,
                    color: t.mutedInk,
                  }}
                >
                  {outcome === "none" ? (
                    <>
                      Nothing was saved to the unit. Your changes are kept, so
                      you can try again.
                    </>
                  ) : (
                    <>
                      {outcome === "all"
                        ? "Changes are now on the unit"
                        : "The presets that saved are changed on the unit"}{" "}
                      and can’t be undone from here. To roll back, restore your
                      last{" "}
                      <strong style={{ color: t.ink2 }}>Pro Control</strong>{" "}
                      backup.
                      {outcome === "partial" &&
                        " The others keep your changes, so you can try again."}
                    </>
                  )}
                </span>
              </div>
            </div>
          ) : (
            <>
              <div
                style={{
                  fontFamily: t.serif,
                  fontSize: t.fsSubhead,
                  color: t.ink,
                }}
              >
                Saving to the unit…
              </div>
              <div
                style={{
                  display: "flex",
                  alignItems: "baseline",
                  justifyContent: "space-between",
                  margin: `${String(t.space5)}px 0 ${String(t.space4)}px`,
                }}
              >
                <span
                  style={{
                    fontFamily: t.mono,
                    fontSize: t.fsUi,
                    color: t.ink2,
                  }}
                >
                  Updating preset {String(Math.min(doneCount + 1, total))} of{" "}
                  {String(total)}…
                </span>
                <span
                  style={{
                    fontFamily: t.mono,
                    fontSize: t.fsMeta,
                    color: t.faint,
                  }}
                >
                  saves automatically
                </span>
              </div>
              <ProgressBar percent={pct} height={5} />
            </>
          )}
        </div>

        <div>
          {slots.map((s) => {
            const res = results.get(s);
            const isActive = !done && s === activeSlot;
            const status: "active" | "queued" | "done" | "error" = res
              ? res.outcome === "error"
                ? "error"
                : "done"
              : isActive
                ? "active"
                : "queued";
            return (
              <div
                key={s}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: t.space5,
                  padding: `${String(t.space4)}px ${String(t.space4)}px`,
                }}
              >
                <span
                  style={{
                    width: 16,
                    display: "inline-flex",
                    justifyContent: "center",
                  }}
                >
                  {status === "active" && (
                    <Spinner size={13} stroke={t.sevWarn} />
                  )}
                  {status === "queued" && <Dot size={6} color={t.faint} />}
                  {status === "done" && (
                    <Icon
                      name="check"
                      size={13}
                      stroke={t.good}
                      strokeWidth={2.2}
                    />
                  )}
                  {status === "error" && (
                    <Icon name="warn-tri" size={13} stroke={t.warn} />
                  )}
                </span>
                <span
                  style={{
                    fontFamily: t.mono,
                    fontSize: t.fsMeta,
                    color: t.faint,
                    width: 28,
                  }}
                >
                  {slotLabel(s)}
                </span>
                <span
                  style={{
                    fontFamily: t.serif,
                    fontSize: t.fsName2,
                    color: status === "queued" ? t.mutedInk : t.ink,
                    flex: 1,
                    minWidth: 0,
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                >
                  {nameOf(s)}
                </span>
                {status === "done" && (
                  <span
                    style={{
                      fontFamily: t.mono,
                      fontSize: t.fsMicro,
                      color: t.good,
                    }}
                  >
                    {res?.outcome === "skipped" ? "no change" : "updated"}
                  </span>
                )}
                {status === "error" && (
                  <span
                    title={res?.detail}
                    style={{
                      fontFamily: t.mono,
                      fontSize: t.fsMicro,
                      color: t.warn,
                    }}
                  >
                    error
                  </span>
                )}
              </div>
            );
          })}
        </div>
      </DialogBody>

      <DialogFooter>
        {done ? (
          <Button variant="primary" small onClick={onDone}>
            Done
          </Button>
        ) : (
          <span
            style={{
              fontFamily: t.sans,
              fontSize: t.fsUi,
              color: t.faint,
              padding: `${String(t.space4)}px ${String(t.space2)}px`,
            }}
          >
            Writing to the unit. Keep it connected.
          </span>
        )}
      </DialogFooter>
    </Dialog>
  );
}

export default SaveOverlay;
