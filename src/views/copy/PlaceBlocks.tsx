// src/views/copy/PlaceBlocks.tsx — Step 2: edit each target's path + save.
//
// Top: the reference strip (its full path, read-only — the blocks you can copy). Then a
// targets header (count, plus an amber "show only over-budget" filter when any target is
// over the CPU cap), the scrollable list of target cards, and the action bar (Back + a
// gating hint or the edited count, then the "I've backed up with Pro Control" checkbox +
// the Save button, which is disabled until there's an edit, nothing's over budget, AND
// the backup box is ticked).

import { useMemo, useState, type Dispatch } from "react";
import { useTheme } from "../../theme/ThemeContext";
import { Button } from "../../ui/primitives";
import { BackupAckLabel } from "../../ui/BackupAckLabel";
import { Icon } from "../../ui/Icon";
import { ActionBar } from "../../ui/ActionBar";
import { Tag } from "../../ui/Tag";
import { slotLabel } from "../../lib/format";
import { CopyPath } from "./CopyPath";
import { TargetEditCard } from "./TargetEditCard";
import { OnUnitChip } from "./OnUnitChip";
import { editGraphFromActive, originBlocks, type EditMap } from "./copyModel";
import type { CopyPreset } from "./useCopyLibrary";
import type { CopyAction, OpenBlock } from "./copyFlow";

/** The derived Save gating (CopyView computes it once per edit). */
export interface SaveGate {
  changedCount: number;
  /** Slots whose staged edit violates a firmware cap (CPU or a block-count rule) —
   *  they block the save until fixed. */
  blockedSlots: number[];
  saveBlocked: boolean;
  hint: string | null;
}

export interface PlaceBlocksProps {
  from: CopyPreset;
  targets: number[];
  edit: EditMap;
  open: OpenBlock | null;
  backedUp: boolean;
  gate: SaveGate;
  /** Offline undo/redo of the staged edits (local — never touches the unit). */
  canUndo: boolean;
  canRedo: boolean;
  nameOf: (slot: number) => string;
  onSave: () => void;
  /** The Copy flow's reducer — every Step-2 interaction is one action. */
  dispatch: Dispatch<CopyAction>;
}

export function PlaceBlocks({
  from,
  targets,
  edit,
  open,
  backedUp,
  gate,
  canUndo,
  canRedo,
  nameOf,
  onSave,
  dispatch,
}: PlaceBlocksProps) {
  const { t } = useTheme();
  const [onlyOver, setOnlyOver] = useState(false);

  const refGraph = useMemo(() => editGraphFromActive(from.graph), [from.graph]);
  const origin = useMemo(() => originBlocks(from.graph), [from.graph]);

  const { changedCount, blockedSlots, saveBlocked, hint } = gate;
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
          padding: `${String(t.space5)}px ${String(t.space8)}px`,
          borderBottom: `0.5px solid ${t.hairline}`,
          background: t.bgAlt,
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: t.space5,
            marginBottom: t.space1,
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
          padding: `${String(t.space4)}px ${String(t.space8)}px ${String(t.space3)}px`,
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: t.space5,
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
          <Button
            variant="bare"
            aria-pressed={showOver}
            onClick={() => {
              setOnlyOver((v) => !v);
            }}
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: t.space3,
              fontFamily: t.sans,
              fontSize: t.fsLabel,
              color: t.sevWarn,
              border: `0.5px solid ${t.sevWarnChipBorder}`,
              background: t.sevWarnChipSoft,
              borderRadius: t.rPill,
              padding: `${String(t.space2)}px ${String(t.space5)}px`,
              whiteSpace: "nowrap",
            }}
          >
            <Icon name="warn-tri" size={12} stroke={t.sevWarn} />
            {`${String(blockedSlots.length)} can't save`} ·{" "}
            {showOver ? "Show all" : "Show only these"}
          </Button>
        )}
      </div>

      {/* targets list */}
      <div
        style={{
          flex: 1,
          minHeight: 0,
          overflowY: "auto",
          padding: `0 ${String(t.space8)}px ${String(t.space7)}px`,
          display: "flex",
          flexDirection: "column",
          gap: t.space4,
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
                dispatch({ type: "tapBlock", slot, uid });
              }}
              onRemove={(uid) => {
                dispatch({ type: "remove", slot, uid });
              }}
              onApply={(uid, mode, model) => {
                dispatch({ type: "apply", slot, uid, mode, model });
              }}
              onClose={() => {
                dispatch({ type: "closeEditor" });
              }}
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
              gap: t.space5,
              minWidth: 0,
            }}
          >
            <Button
              variant="bare"
              onClick={() => {
                dispatch({ type: "back" });
              }}
              style={{
                display: "inline-flex",
                alignItems: "center",
                gap: t.space3,
                fontFamily: t.sans,
                fontSize: t.fsControl,
                color: t.ink2,
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
            </Button>
            <span style={{ width: 1, height: 22, background: t.hairline }} />
            <div
              title="Undo / redo your edits — nothing is on the unit yet"
              style={{ display: "flex", alignItems: "center", gap: t.space3 }}
            >
              {(
                [
                  { icon: "undo", label: "Undo", enabled: canUndo },
                  { icon: "redo", label: "Redo", enabled: canRedo },
                ] as const
              ).map((b) => (
                <Button
                  key={b.icon}
                  variant="bare"
                  aria-label={b.label}
                  disabled={!b.enabled}
                  onClick={() => {
                    dispatch({ type: b.icon });
                  }}
                  style={{
                    display: "inline-flex",
                    alignItems: "center",
                    justifyContent: "center",
                    height: 30,
                    padding: `0 ${String(t.space5)}px`,
                    borderRadius: t.rBtn,
                    opacity: b.enabled ? 1 : 0.35,
                  }}
                >
                  <Icon name={b.icon} size={15} stroke={t.ink2} />
                </Button>
              ))}
            </div>
            <span style={{ width: 1, height: 22, background: t.hairline }} />
            {saveBlocked && hint != null ? (
              <span
                style={{
                  display: "inline-flex",
                  alignItems: "center",
                  gap: t.space4,
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
              gap: t.space7,
              flexShrink: 0,
            }}
          >
            <BackupAckLabel
              checked={backedUp}
              onChange={(value) => {
                dispatch({ type: "setBackedUp", value });
              }}
            />
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
