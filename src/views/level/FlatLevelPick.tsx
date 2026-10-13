// src/views/level/FlatLevelPick.tsx — the Set up step's ONE-DROPDOWN leveling-handle
// picker (D2) — the design-1a redesign's "Knob" control — with full any-block/any-param
// capability: every candidate `blockLevelGroups.ts` offers shows up here, in rank
// order, with the "Recommended" / "may change the tone" / "can only lower" /
// disabled-with-reason notes, and the DANGER-rule stale-handle guard (a stored pick
// the candidate list doesn't cover renders VERBATIM + a warning, never a silent
// fallback to the pseudo-option or the first row).
//
// One row per CANDIDATE (not per block), grouped by block with a small mono header
// when a preset carries more than one leveling-relevant block, so a many-block
// preset's list stays scannable without a second click-through.
//
// Built on the Pick family's shared machinery: `usePickAnchor`, `PickPortalMenu`,
// `PickWarnNote`, and the ONE trigger (`PickTrigger`) and ONE row (`PickListRow` —
// "rich" for the candidates, "compact" for the pseudo default row) `Pick` uses too.

import { useContext } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Tag } from "../../ui/Tag";
import { blockArtTile } from "../../models/blockArt";
import { paramLabel } from "./leveling";
import {
  groupByBlock,
  recommendedBlock,
  blockKeyOf,
  type BlockLevelCandidate,
  type BlockLevelHandle,
} from "./blockLevelGroups";
import { DialogCardCtx } from "../overlays/wizardContext";
import { PickPortalMenu } from "../overlays/PickPortalMenu";
import { PickTrigger } from "../overlays/PickTrigger";
import { PickWarnNote } from "../overlays/PickWarnNote";
import { usePickAnchor } from "../overlays/usePickAnchor";
import { PickListRow } from "../overlays/PickListRow";

export type { BlockLevelCandidate, BlockLevelHandle };

export type FlatLevelFetch =
  | { status: "unfetched" | "loading" | "error" }
  | { status: "resolved"; list: BlockLevelCandidate[] };

export interface FlatLevelPickProps {
  /** Pseudo first entry's label ("Preset level" / "Amp output level"). Omit for
   *  footswitch rows — every FS row must carry a real handle. */
  pseudoLabel?: string;
  /** `null` = the pseudo-option (when offered) or "not yet resolved". */
  handle: BlockLevelHandle | null;
  onHandleChange: (h: BlockLevelHandle | null) => void;
  candidates: FlatLevelFetch;
  /** Fire the lazy per-preset fetch (Base/Scene rows only). Idempotent. A no-op prop
   *  for footswitch rows, whose candidates are already in hand. */
  onOpen: () => void;
}

const candidateKey = (c: {
  groupId: string;
  nodeId: string;
  parameterId: string;
}): string => `${c.groupId}:${c.nodeId}:${c.parameterId}`;

export function FlatLevelPick({
  pseudoLabel,
  handle,
  onHandleChange,
  candidates,
  onOpen,
}: FlatLevelPickProps) {
  const { t } = useTheme();
  const cardRef = useContext(DialogCardCtx);
  const resolved = candidates.status === "resolved";
  const list = resolved ? candidates.list : [];
  const blocks = groupByBlock(list);

  const handleKey = handle ? candidateKey(handle) : null;
  const matched = handleKey
    ? list.find((c) => candidateKey(c) === handleKey)
    : undefined;
  // DANGER-rule guard: a stored handle the (resolved) candidate list doesn't cover
  // renders verbatim + a warning, never a silent fallback to the pseudo-option or
  // the first row.
  const stale = handle != null && resolved && matched == null;

  const recommended = recommendedBlock(blocks);
  const recommendedKey = recommended ? candidateKey(recommended) : null;

  const triggerLabel = (() => {
    if (handle == null) return pseudoLabel ?? "Choose a control";
    if (matched) {
      const art = blockArtTile(matched.fenderId);
      return `${art.fullName ?? art.name} — ${paramLabel(matched.parameterId)}`;
    }
    if (resolved) return `${handle.nodeId} · ${paramLabel(handle.parameterId)}`;
    return paramLabel(handle.parameterId);
  })();

  const { open, anchor, pos, cardEl, menuRef, triggerRef, openMenu, close } =
    usePickAnchor(cardRef, {
      onOpen,
      contentKey: `${candidates.status}:${String(list.length)}`,
    });

  const pick = (c: BlockLevelCandidate) => {
    onHandleChange({
      groupId: c.groupId,
      nodeId: c.nodeId,
      parameterId: c.parameterId,
    });
    close();
  };

  const candidateRow = (c: BlockLevelCandidate) => {
    const key = candidateKey(c);
    const on = key === handleKey;
    const rec = key === recommendedKey;
    const loud = c.paramClass !== "wet_mix";
    const art = blockArtTile(c.fenderId);
    return (
      <PickListRow
        key={key}
        density="rich"
        pickKey={key}
        label={`${art.fullName ?? art.name} — ${paramLabel(c.parameterId)}`}
        selected={on}
        disabled={c.disabled}
        disabledTitle={c.disabled ? c.disabledTitle : undefined}
        onPick={() => {
          pick(c);
        }}
        note={
          c.disabled ? (
            <PickWarnNote>{c.disabledTitle}</PickWarnNote>
          ) : rec ? (
            <Tag tone="good" uppercase>
              {loud ? "Recommended - loudness only" : "Recommended"}
            </Tag>
          ) : !loud ? (
            <PickWarnNote>may change the tone</PickWarnNote>
          ) : c.lowersOnly ? (
            <span
              style={{ fontFamily: t.sans, fontSize: 10.5, color: t.mutedInk }}
            >
              can only lower
            </span>
          ) : undefined
        }
      />
    );
  };

  return (
    <PickTrigger
      triggerRef={triggerRef}
      open={open}
      warn={stale}
      label={triggerLabel}
      title="Choose this sound's leveling control"
      onClick={openMenu}
      face="sans"
    >
      {open && anchor && cardEl && (
        <PickPortalMenu
          cardEl={cardEl}
          menuRef={menuRef}
          left={pos ? pos.left : anchor.left}
          top={pos ? pos.top : anchor.below}
          visible={pos != null}
          minWidth={Math.max(anchor.width, 280)}
          onClose={close}
        >
          {pseudoLabel != null && (
            <PickListRow
              label={`${pseudoLabel} (default)`}
              selected={handle == null}
              check
              onPick={() => {
                onHandleChange(null);
                close();
              }}
            />
          )}

          {candidates.status === "loading" && (
            <div
              style={{
                padding: t.space4,
                fontFamily: t.sans,
                fontSize: 11,
                color: t.mutedInk,
              }}
            >
              Loading controls…
            </div>
          )}
          {candidates.status === "error" && (
            <div
              style={{
                padding: t.space4,
                fontFamily: t.sans,
                fontSize: 11,
                color: t.sevWarn,
              }}
            >
              Couldn’t read this preset’s controls.
            </div>
          )}
          {blocks.map((group) => {
            const first = group.length > 0 ? group[0] : undefined;
            if (!first) return null;
            const art = blockArtTile(first.fenderId);
            return (
              <div
                key={blockKeyOf(first)}
                role="group"
                aria-label={art.fullName ?? art.name}
              >
                {blocks.length > 1 && (
                  <div
                    style={{
                      padding: `${String(t.space3)}px ${String(t.space4)}px 0`,
                      fontFamily: t.mono,
                      fontSize: 9.5,
                      letterSpacing: "0.08em",
                      textTransform: "uppercase",
                      color: t.faint,
                    }}
                  >
                    {art.fullName ?? art.name}
                  </div>
                )}
                {group.map(candidateRow)}
              </div>
            );
          })}
          {stale && (
            <div style={{ padding: t.space4 }}>
              <PickWarnNote>
                stored pick no longer offered — pick again
                {pseudoLabel ? " or use the default" : ""}.
              </PickWarnNote>
            </div>
          )}
        </PickPortalMenu>
      )}
    </PickTrigger>
  );
}
