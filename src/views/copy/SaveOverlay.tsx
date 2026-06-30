// src/views/copy/SaveOverlay.tsx — the Save progress → done overlay.
//
// Driven by the REAL `copy_apply` run (the channel streams one result per preset), NOT a
// synthetic timer: `results` fills as each preset commits, `activeSlot` is the one being
// written. Progress phase lists every target slot with a status icon (spinner / queued
// dot / green check / warn for an error). Done phase shows a green medallion + the count
// + a Pro Control backup-restore note (writes to the unit are not undoable from here —
// undo/redo is offline, pre-save only) and a single Done button.

import { useTheme } from "../../theme/ThemeContext";
import { Dialog, DialogBody, DialogFooter } from "../../ui/Dialog";
import { Icon } from "../../ui/Icon";
import { Button } from "../../ui/primitives";
import { ProgressBar } from "../../ui/ProgressBar";
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
  onDone: () => void;
}

export function SaveOverlay({
  slots,
  nameOf,
  results,
  activeSlot,
  done,
  onDone,
}: SaveOverlayProps) {
  const { t } = useTheme();
  const total = slots.length;
  const doneCount = results.size;
  const pct = total === 0 ? 100 : Math.round((doneCount / total) * 100);
  const errored = [...results.values()].filter(
    (r) => r.outcome === "error",
  ).length;

  return (
    <Dialog size="sm" zIndex={60} label="Saving to the unit">
      <DialogBody>
        <div style={{ paddingBottom: 14 }}>
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
                  background: t.goodSoft,
                  border: `0.5px solid ${t.goodBorder}`,
                }}
              >
                <Icon
                  name="check"
                  size={24}
                  stroke={t.good}
                  strokeWidth={2.2}
                />
              </span>
              <div
                style={{
                  fontFamily: t.serif,
                  fontSize: t.fsTitle,
                  color: t.ink,
                  marginTop: 12,
                }}
              >
                Saved to the unit.
              </div>
              <div
                style={{
                  fontFamily: t.sans,
                  fontSize: t.fsBody2,
                  color: t.mutedInk,
                  marginTop: 5,
                }}
              >
                {String(total - errored)} preset
                {total - errored === 1 ? "" : "s"} updated
                {errored > 0 ? ` · ${String(errored)} could not be saved` : ""}.
              </div>
              {/* Once written, the unit can't be rolled back from here — the only
                  recovery is restoring a Pro Control backup (undo/redo was offline,
                  pre-save only). */}
              <div
                style={{
                  display: "flex",
                  alignItems: "flex-start",
                  gap: 10,
                  marginTop: 16,
                  padding: "11px 13px",
                  borderRadius: t.rMd,
                  background: t.bgAlt,
                  textAlign: "left",
                }}
              >
                <span style={{ flexShrink: 0, marginTop: 1 }}>
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
                  Changes are now on the unit and can’t be undone from here. To
                  roll back, restore your last{" "}
                  <strong style={{ color: t.ink2 }}>Pro Control</strong> backup.
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
                  margin: "11px 0 8px",
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
                  one preset at a time
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
                  gap: 11,
                  padding: "7px 8px",
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
                    <span
                      className="tmp-spin"
                      style={{ display: "inline-flex" }}
                    >
                      <Icon name="spinner" size={13} stroke={t.sevWarn} />
                    </span>
                  )}
                  {status === "queued" && (
                    <span
                      style={{
                        width: 6,
                        height: 6,
                        borderRadius: t.rPill,
                        background: t.faint,
                      }}
                    />
                  )}
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
              padding: "8px 4px",
            }}
          >
            Writing to the unit — please keep it connected.
          </span>
        )}
      </DialogFooter>
    </Dialog>
  );
}

export default SaveOverlay;
