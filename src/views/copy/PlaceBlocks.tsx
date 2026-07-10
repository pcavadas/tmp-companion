// src/views/copy/PlaceBlocks.tsx — Step 2: edit each target's path + save.
//
// Top: the reference strip (its full path, read-only — the blocks you can copy). Then a
// targets header (count, plus an amber "show only over-budget" filter when any target is
// over the CPU cap), the scrollable list of target cards, and the action bar (Back + a
// gating hint or the edited count, then the "I've backed up with Pro Control" checkbox +
// the Save button, which is disabled until there's an edit, nothing's over budget, AND
// the backup box is ticked).

import { useMemo, useState } from "react";
import { useTheme } from "../../theme/ThemeContext";
import { Button } from "../../ui/primitives";
import { BackupAckLabel } from "../../ui/BackupAckLabel";
import { Icon } from "../../ui/Icon";
import { ActionBar } from "../../ui/ActionBar";
import { Tag } from "../../ui/Tag";
import { slotLabel } from "../../lib/format";
import { CopyPath } from "./CopyPath";
import { TargetEditCard } from "./TargetEditCard";
import { OnUnitChip } from "./copyBits";
import { type EditMode } from "./BlockEditor";
import { editGraphFromActive, originBlocks, type EditMap } from "./copyModel";
import type { CopyPreset } from "./useCopyLibrary";

export interface PlaceBlocksProps {
  from: CopyPreset;
  targets: number[];
  edit: EditMap;
  open: { slot: number; uid: string } | null;
  changedCount: number;
  /** Slots whose staged edit violates a firmware cap (CPU or a block-count rule) —
   *  they block the save until fixed. */
  blockedSlots: number[];
  saveBlocked: boolean;
  hint: string | null;
  backedUp: boolean;
  setBackedUp: (v: boolean) => void;
  nameOf: (slot: number) => string;
  onBack: () => void;
  onSave: () => void;
  onTapBlock: (slot: number, uid: string) => void;
  onApply: (slot: number, uid: string, mode: EditMode, model: string) => void;
  onRemove: (slot: number, uid: string) => void;
  onClose: () => void;
  /** Offline undo/redo of the staged edits (local — never touches the unit). */
  onUndo: () => void;
  onRedo: () => void;
  canUndo: boolean;
  canRedo: boolean;
}

export function PlaceBlocks({
  from,
  targets,
  edit,
  open,
  changedCount,
  blockedSlots,
  saveBlocked,
  hint,
  backedUp,
  setBackedUp,
  nameOf,
  onBack,
  onSave,
  onTapBlock,
  onApply,
  onRemove,
  onClose,
  onUndo,
  onRedo,
  canUndo,
  canRedo,
}: PlaceBlocksProps) {
  const { t } = useTheme();
  const [onlyOver, setOnlyOver] = useState(false);

  const refGraph = useMemo(() => editGraphFromActive(from.graph), [from.graph]);
  const origin = useMemo(() => originBlocks(from.graph), [from.graph]);

  const blockedSet = new Set(blockedSlots);
  const showOver = onlyOver && blockedSlots.length > 0;
  const visible = showOver ? targets.filter((s) => blockedSet.has(s)) : targets;

  const saveDisabled = saveBlocked || !backedUp;

  return (
    <div
      style={{
        flex: 1,
        minHeight: 0,
        display: "flex",
        flexDirection: "column",
      }}
    >
      {/* reference */}
      <div
        style={{
          flexShrink: 0,
          padding: "11px 18px",
          borderBottom: `0.5px solid ${t.hairline}`,
          background: t.bgAlt,
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 10,
            marginBottom: 2,
          }}
        >
          <Tag size="md" tone="accent">
            {slotLabel(from.slot)}
          </Tag>
          <span
            style={{ fontFamily: t.serif, fontSize: t.fsCard, color: t.ink }}
          >
            {from.name}
          </span>
          {from.onUnit && <OnUnitChip />}
          <span
            style={{
              fontFamily: t.sans,
              fontSize: t.fsLabel,
              color: t.mutedInk,
            }}
          >
            — the blocks you can copy. Tap a block in a preset below to use one.
          </span>
        </div>
        <CopyPath graph={refGraph} />
      </div>

      {/* targets header */}
      <div
        style={{
          flexShrink: 0,
          padding: "9px 18px 6px",
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: 10,
        }}
      >
        <span
          style={{
            fontFamily: t.mono,
            fontSize: t.fsMicro,
            letterSpacing: t.lsTag,
            textTransform: "uppercase",
            color: showOver ? t.sevWarn : t.faint,
          }}
        >
          {showOver
            ? `${String(visible.length)} can't save`
            : `Editing ${String(targets.length)} preset${targets.length === 1 ? "" : "s"} — each keeps its own path`}
        </span>
        {blockedSlots.length > 0 && (
          <span
            role="button"
            onClick={() => {
              setOnlyOver((v) => !v);
            }}
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: 6,
              fontFamily: t.sans,
              fontSize: t.fsLabel,
              color: t.sevWarn,
              border: "0.5px solid rgba(176,125,28,0.4)",
              background: "rgba(176,125,28,0.10)",
              borderRadius: t.rPill,
              padding: "3px 10px",
              cursor: "pointer",
              whiteSpace: "nowrap",
            }}
          >
            <Icon name="warn-tri" size={12} stroke={t.sevWarn} />
            {`${String(blockedSlots.length)} can't save`} ·{" "}
            {showOver ? "Show all" : "Show only these"}
          </span>
        )}
      </div>

      {/* targets list */}
      <div
        style={{
          flex: 1,
          minHeight: 0,
          overflowY: "auto",
          padding: "0 16px 14px",
          display: "flex",
          flexDirection: "column",
          gap: 9,
        }}
      >
        {visible.map((slot) => {
          const e = edit[slot];
          if (!e) return null;
          return (
            <TargetEditCard
              key={slot}
              slot={slot}
              name={nameOf(slot)}
              edit={e}
              openUid={open?.slot === slot ? open.uid : null}
              fromName={from.name}
              origin={origin}
              onTapBlock={(uid) => {
                onTapBlock(slot, uid);
              }}
              onRemove={(uid) => {
                onRemove(slot, uid);
              }}
              onApply={(uid, mode, model) => {
                onApply(slot, uid, mode, model);
              }}
              onClose={onClose}
            />
          );
        })}
      </div>

      {/* draft bar */}
      <ActionBar
        left={
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: 11,
              minWidth: 0,
            }}
          >
            <span
              role="button"
              onClick={onBack}
              style={{
                display: "inline-flex",
                alignItems: "center",
                gap: 5,
                fontFamily: t.sans,
                fontSize: t.fsControl,
                color: t.ink2,
                cursor: "pointer",
              }}
            >
              <span style={{ display: "inline-flex", transform: "scaleX(-1)" }}>
                <Icon
                  name="chev-right"
                  size={14}
                  stroke={t.ink2}
                  strokeWidth={2}
                />
              </span>
              Back
            </span>
            <span style={{ width: 1, height: 22, background: t.hairline }} />
            <div
              title="Undo / redo your edits — nothing is on the unit yet"
              style={{ display: "flex", alignItems: "center", gap: 6 }}
            >
              {(
                [
                  { icon: "undo", on: onUndo, enabled: canUndo },
                  { icon: "redo", on: onRedo, enabled: canRedo },
                ] as const
              ).map((b) => (
                <span
                  key={b.icon}
                  role="button"
                  onClick={b.enabled ? b.on : undefined}
                  style={{
                    display: "inline-flex",
                    alignItems: "center",
                    justifyContent: "center",
                    height: 30,
                    padding: "0 10px",
                    borderRadius: t.rBtn,
                    opacity: b.enabled ? 1 : 0.35,
                    pointerEvents: b.enabled ? "auto" : "none",
                    cursor: b.enabled ? "pointer" : "default",
                  }}
                >
                  <Icon name={b.icon} size={15} stroke={t.ink2} />
                </span>
              ))}
            </div>
            <span style={{ width: 1, height: 22, background: t.hairline }} />
            {saveBlocked && hint != null ? (
              <span
                style={{
                  display: "inline-flex",
                  alignItems: "center",
                  gap: 7,
                  fontFamily: t.sans,
                  fontSize: t.fsControl,
                  color: t.sevWarn,
                }}
              >
                <Icon name="warn-tri" size={14} stroke={t.sevWarn} />
                {hint}
              </span>
            ) : (
              <span
                style={{
                  fontFamily: t.sans,
                  fontSize: t.fsControl,
                  color: t.ink2,
                }}
              >
                <strong style={{ color: t.ink }}>{String(changedCount)}</strong>{" "}
                of {String(targets.length)} presets edited
              </span>
            )}
          </div>
        }
        right={
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: 14,
              flexShrink: 0,
            }}
          >
            <BackupAckLabel checked={backedUp} onChange={setBackedUp} />
            <Button
              variant="primary"
              icon="check"
              disabled={saveDisabled}
              onClick={onSave}
            >
              Save to the unit
            </Button>
          </div>
        }
      />
    </div>
  );
}

export default PlaceBlocks;
