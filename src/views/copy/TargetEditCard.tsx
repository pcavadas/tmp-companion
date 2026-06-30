// src/views/copy/TargetEditCard.tsx — one editable target preset in Step 2.
//
// Header: slot + name + an "edited" chip (when the preset differs from its original) +
// the CPU meter. Below: the target's editable signal path (tap a tile → the inline
// editor opens in place of the hint line). The card border turns amber when this
// preset's CPU is over budget.

import { useMemo } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { slotLabel } from "../../lib/format";
import { CPU_BUDGET } from "../../models/cpu";
import { CopyPath } from "./CopyPath";
import { BlockEditor, type EditMode } from "./BlockEditor";
import { CpuMeter } from "./CpuMeter";
import {
  cpuOfGraph,
  findBlock,
  isEdited,
  type OriginBlock,
  type PresetEdit,
} from "./copyModel";

export interface TargetEditCardProps {
  slot: number;
  name: string;
  edit: PresetEdit;
  /** The open block's uid in THIS card, or null when nothing here is open. */
  openUid: string | null;
  fromName: string;
  origin: OriginBlock[];
  onTapBlock: (uid: string) => void;
  onRemove: (uid: string) => void;
  onApply: (uid: string, mode: EditMode, model: string) => void;
  onClose: () => void;
}

export function TargetEditCard({
  slot,
  name,
  edit,
  openUid,
  fromName,
  origin,
  onTapBlock,
  onRemove,
  onApply,
  onClose,
}: TargetEditCardProps) {
  const { t } = useTheme();
  // Re-summed only when this card's lanes change (an edit op), not on every parent
  // render (e.g. opening/closing another card's inline editor).
  const cpu = useMemo(() => cpuOfGraph(edit.graph), [edit.graph]);
  const dirty = useMemo(() => isEdited(edit), [edit]);
  const over = cpu > CPU_BUDGET;
  const openBlock = openUid != null ? findBlock(edit.graph, openUid) : null;

  return (
    <div
      // e2e hook: scope block-tile selectors to one target's card (multi-preset edit).
      data-target-card={name}
      style={{
        padding: "11px 14px",
        borderRadius: t.rPopover,
        border: `0.5px solid ${over ? "rgba(176,125,28,0.5)" : t.hairlineStrong}`,
        background: t.bg,
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 9 }}>
        <span
          style={{ fontFamily: t.mono, fontSize: t.fsMeta, color: t.faint }}
        >
          {slotLabel(slot)}
        </span>
        <span style={{ fontFamily: t.serif, fontSize: t.fsName, color: t.ink }}>
          {name}
        </span>
        {dirty && (
          <span
            style={{
              fontFamily: t.mono,
              fontSize: t.fsTag,
              letterSpacing: t.lsTag,
              textTransform: "uppercase",
              color: t.accentDeep,
              border: `0.5px solid ${t.accentBorder}`,
              background: t.accentSoft,
              borderRadius: t.rSm,
              padding: "1px 5px",
            }}
          >
            edited
          </span>
        )}
        <span style={{ flex: 1 }} />
        <CpuMeter value={cpu} />
      </div>

      <CopyPath graph={edit.graph} onTap={onTapBlock} selectedUid={openUid} />

      {openUid == null || openBlock == null ? (
        <div
          style={{
            fontFamily: t.sans,
            fontSize: t.fsLabel,
            color: t.faint,
            padding: "2px 2px 0",
          }}
        >
          Tap a block to replace, remove, or add one next to it.
        </div>
      ) : (
        <BlockEditor
          block={openBlock}
          fromName={fromName}
          origin={origin}
          onRemove={() => {
            onRemove(openUid);
          }}
          onApply={(mode, model) => {
            onApply(openUid, mode, model);
          }}
          onClose={onClose}
        />
      )}
    </div>
  );
}

export default TargetEditCard;
