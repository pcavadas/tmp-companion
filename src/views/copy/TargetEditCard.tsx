// src/views/copy/TargetEditCard.tsx — one target preset in Place blocks (look B).
//
// A white panel: the name, its slot, "Changed" (accent) once it differs from what is on
// your Tone Master Pro, and the CPU readout at the right ("Over" past the budget). Under it
// the preset's signal path. While a source block is chosen, every gap of the path holds a
// "+" that places that block there; when the block would break a firmware limit anywhere
// in this preset the "+" are greyed and a caution line under the path says why. A click
// on a block opens its menu (Replace with the chosen block, Remove). When the edit as it
// stands breaks a limit, the panel's edge turns danger and a line under the path says
// why; Save then waits.

import { useCallback, useMemo, useRef } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Badge } from "../../ui/Badge";
import { Icon } from "../../ui/Icon";
import { SlotLabel } from "../../ui/SlotLabel";
import { CopyPath, type CopyGaps } from "./CopyPath";
import { BlockMenu } from "./BlockMenu";
import { CpuMeter } from "./CpuMeter";
import { COPY_TEXT, REASON_TEXT, blockName, gapReason } from "./copyText";
import {
  cpuOfGraph,
  findBlock,
  isEdited,
  type EditMode,
  type PresetEdit,
} from "./copyModel";
import { baseCounts, checkOp, type BlockEditReason } from "./validateBlockEdit";
import type { PickedBlock } from "./copyFlow";

export interface TargetEditCardProps {
  slot: number;
  name: string;
  edit: PresetEdit;
  /** The block whose menu is open in THIS card, or null. */
  openUid: string | null;
  /** The source block chosen to copy, or null. */
  pick: PickedBlock | null;
  /** Why this edit can't be saved as it stands (a firmware limit), or null. */
  blocked: BlockEditReason | null;
  onTapBlock: (uid: string) => void;
  onRemove: (uid: string) => void;
  onApply: (uid: string, mode: EditMode, model: string) => void;
  onCloseMenu: () => void;
}

export function TargetEditCard({
  slot,
  name,
  edit,
  openUid,
  pick,
  blocked,
  onTapBlock,
  onRemove,
  onApply,
  onCloseMenu,
}: TargetEditCardProps) {
  const { t } = useTheme();
  const cardRef = useRef<HTMLDivElement>(null);
  // Re-summed only when this card's blocks change (an edit op), not on every parent
  // render (e.g. opening another card's menu).
  const cpu = useMemo(() => cpuOfGraph(edit.graph), [edit.graph]);
  const dirty = useMemo(() => isEdited(edit), [edit]);
  // The preset's standing against each firmware cap, before the next op.
  const counts = useMemo(() => baseCounts(edit.graph), [edit.graph]);
  const openBlock = openUid != null ? findBlock(edit.graph, openUid) : null;

  // An insert frees nothing, so one check covers every gap of this preset.
  const gapBlocked = pick ? checkOp(counts, pick.model, "before") : null;
  const pickName = pick ? blockName(pick.model) : "";
  const gaps = useMemo<CopyGaps | undefined>(
    () =>
      pick
        ? {
            presetName: name,
            disabled: gapBlocked != null,
            onPlace: (uid, mode) => {
              onApply(uid, mode, pick.model);
            },
          }
        : undefined,
    [pick, name, gapBlocked, onApply],
  );

  // Replace is offered only while a block is chosen and the swap breaks no limit.
  const canReplace =
    pick != null &&
    openBlock != null &&
    checkOp(counts, pick.model, "replace", {
      anchor: { model: openBlock.model, dualCab: openBlock.cabSim2Enabled },
    }) == null;

  // The open block's tile (the strip draws it pressed): the menu's anchor.
  const tileOf = useCallback(
    () =>
      cardRef.current?.querySelector(
        '[data-block-tile][aria-pressed="true"]',
      ) ?? null,
    [],
  );

  const note: { text: string; tone: "danger" | "caution" } | null = blocked
    ? { text: REASON_TEXT[blocked], tone: "danger" }
    : gapBlocked
      ? { text: gapReason(gapBlocked, pickName), tone: "caution" }
      : null;

  return (
    <div
      ref={cardRef}
      // e2e hook: scope block-tile selectors to one target's card (multi-preset edit).
      data-target-card={name}
      style={{
        display: "flex",
        flexDirection: "column",
        gap: t.sp8 + t.sp2,
        padding: `${String(t.sp12)}px ${String(t.sp16)}px`,
        background: t.surface,
        border: `1px solid ${blocked ? t.danger : t.border}`,
        borderRadius: t.radiusGroup,
        fontFamily: t.fontSans,
        color: t.text,
        minWidth: 0,
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: t.sp8 }}>
        <span
          style={{
            fontSize: t.fontBody,
            fontWeight: t.weightSemibold,
            whiteSpace: "nowrap",
            overflow: "hidden",
            textOverflow: "ellipsis",
            minWidth: 0,
          }}
        >
          {name}
        </span>
        <SlotLabel index={slot} style={{ flexShrink: 0 }} />
        {dirty && <Badge tone="accent">{COPY_TEXT.changed}</Badge>}
        <span style={{ flex: 1 }} />
        <CpuMeter value={cpu} />
      </div>

      <CopyPath
        graph={edit.graph}
        onTap={onTapBlock}
        selectedUid={openUid}
        gaps={gaps}
      />

      {note && (
        <div
          style={{
            display: "flex",
            alignItems: "flex-start",
            gap: t.sp6,
            fontSize: t.fontCaption,
            lineHeight: `${String(t.lineCaption)}px`,
          }}
        >
          <span style={{ display: "flex", paddingTop: 1.5, flexShrink: 0 }}>
            <Icon
              name={note.tone === "danger" ? "alert" : "warn-tri"}
              size={13}
              stroke={note.tone === "danger" ? t.danger : t.caution}
              strokeWidth={2}
            />
          </span>
          {note.text}
        </div>
      )}

      {openUid != null && openBlock != null && (
        <BlockMenu
          anchor={tileOf}
          replaceName={canReplace ? pickName : null}
          onReplace={() => {
            if (pick) onApply(openUid, "replace", pick.model);
          }}
          onRemove={() => {
            onRemove(openUid);
          }}
          onClose={onCloseMenu}
        />
      )}
    </div>
  );
}
