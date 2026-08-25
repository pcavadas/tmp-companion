// src/views/overlays/BlockLevelPick.tsx — the Set up step's TWO-DROPDOWN leveling-
// handle picker (D2, Part C). ONE component drives all three row kinds, now split
// into a BLOCK dropdown followed by a CONTROL dropdown — the original single flat
// block+param list grew too long once a preset carried many blocks:
//   • Base rows — candidates from `list_level_blocks`; a "Preset level" pseudo-option
//     (the master `presetLevel`) is the default first entry.
//   • Scene rows — candidates from `list_scene_level_handles`'s `allCandidates`; an
//     "Amp output level" pseudo-option (the per-scene amp joint-k path) is the default.
//   • Footswitch rows — candidates are the switch's own `level_params` (no device
//     read — already in hand). NO pseudo-option: every FS row must carry a real
//     handle (the backend removed the verify-only "no handle" row entirely).
// A pseudo-option submits `handle: null` on the wire — for Base/Scene that is a
// DIFFERENT, richer path than any single block param (the backend's own per-scene amp
// auto-pick, or the preset-level path), never just "the first candidate". Picking the
// pseudo-option happens in the BLOCK dropdown (it has no param, so the control
// dropdown never shows for it).
//
// Candidates are grouped BY BLOCK and resolved against a stored handle by
// `../level/blockLevelGroups.ts` — rank ordering, the "best enabled candidate" rule,
// and the DANGER-rule stale guard all live there as plain-data functions, unit-tested
// without rendering anything (`blockLevelGroups.test.ts`); this component composes
// that data into the two-dropdown UI. The BLOCK dropdown lists one row per block
// (`BlockPickRow` — art tile + full name); the single overall-best candidate (the
// first block, in rank
// order, that has an ENABLED candidate — `recommendedBlock`) flags its block
// "Recommended" there. A block whose every candidate is `disabled` (Scene's
// `shared_with_base`/`unknown` scope) stays visible but inert, with the shared reason
// — mirroring the per-candidate disabled note the CONTROL dropdown (`BlockParamRow`)
// already carried. Selecting a DIFFERENT block auto-picks its best ENABLED candidate,
// via the SAME `bestEnabled` the "Recommended" tag reads (fewest clicks: one click
// can pick a block AND land on the right control) — re-selecting the block that
// already holds the stored handle is a no-op — it never silently changes a stored
// pick. The control dropdown is the override: it lists the selected block's own
// params (level-class first) with the SAME per-candidate notes (Recommended / "may
// change the tone" for `wet_mix` / "can only lower" / disabled reason) BlockLevelPick
// has always shown.
//
// DANGER-rule guard (`Pick`/`BlockPick` trap): a stored `handle` the current candidate
// list doesn't cover must render VERBATIM + a warning, never silently fall back to the
// pseudo-option or `candidates[0]`. Split across two dropdowns, the trap now has two
// arms: the stored block itself is missing from the (resolved) candidate list ⇒ the
// BLOCK trigger renders the raw `nodeId` + the param's label verbatim (the control
// dropdown has nothing to show, so it stays hidden — losing it there would silently
// drop the stored param from view, which the trap forbids) with a warning; the block IS
// present but its stored param is gone ⇒ the block trigger renders normally and the
// CONTROL trigger renders the raw param label + "(removed)" with a warning.
//
// Click-only. Reuses the wizard's card-portaled dropdown (`usePickAnchor` /
// `PickPortalMenu`) TWICE — once per dropdown, each with its own `PickTrigger`
// (the shared trigger chrome) + menu.

import { useContext } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { Tag } from "../../ui/Tag";
import { blockArtTile } from "../../models/blockArt";
import { paramLabel } from "../level/leveling";
import {
  groupByBlock,
  groupHead,
  bestEnabled,
  recommendedBlock,
  resolveHandle,
  isDisabledCandidate,
  blockKeyOf,
  type BlockLevelCandidate,
  type BlockLevelHandle,
} from "../level/blockLevelGroups";
import { DialogCardCtx } from "./wizardContext";
import { PickPortalMenu } from "./PickPortalMenu";
import { PickTrigger } from "./PickTrigger";
import { PickWarnNote } from "./PickWarnNote";
import { usePickAnchor } from "./usePickAnchor";
import { BlockPickRow } from "./BlockPickRow";
import { BlockParamRow } from "./BlockParamRow";

export type { BlockLevelCandidate, BlockLevelHandle };

export type BlockLevelFetch =
  | { status: "unfetched" | "loading" | "error" }
  | { status: "resolved"; list: BlockLevelCandidate[] };

export interface BlockLevelPickProps {
  /** Pseudo first entry's label ("Preset level" / "Amp output level"), offered in
   *  the BLOCK dropdown. Omit for footswitch rows — D2: every FS row must carry a
   *  real handle. */
  pseudoLabel?: string;
  /** `null` = the pseudo-option (when offered) or, on a footswitch row, "not yet
   *  resolved" (the row always seeds a real default, so this stays defensive). */
  handle: BlockLevelHandle | null;
  onHandleChange: (h: BlockLevelHandle | null) => void;
  candidates: BlockLevelFetch;
  /** Fire the lazy per-preset fetch (Base/Scene rows only). Idempotent — safe to call
   *  on every open (both dropdowns call it). A no-op prop (`() => undefined`) for
   *  footswitch rows, whose candidates are already in hand. */
  onOpen: () => void;
}

/** The stored handle's block-trigger label: verbatim `nodeId · param label` (never
 *  the catalog name) while the block itself is missing from a resolved list — the
 *  DANGER-rule guard forbids silently swapping in a friendlier string for a handle
 *  that isn't actually confirmed. */
function blockTriggerLabelFor(
  handle: BlockLevelHandle | null,
  pseudoLabel: string | undefined,
  blockArt: ReturnType<typeof blockArtTile> | null,
  resolved: boolean,
): string {
  if (handle == null) return pseudoLabel ?? "Choose a control";
  if (blockArt) return blockArt.fullName ?? blockArt.name;
  if (resolved) return `${handle.nodeId} · ${paramLabel(handle.parameterId)}`;
  return paramLabel(handle.parameterId);
}

/** The stored handle's control-trigger label — computes `paramLabel` once (it was
 *  called twice, one per ternary arm, before this was hoisted out). */
function controlTriggerLabelFor(
  handle: BlockLevelHandle | null,
  matched: BlockLevelCandidate | undefined,
): string {
  if (handle == null) return "";
  const label = paramLabel(handle.parameterId);
  return matched ? label : `${label} (removed)`;
}

export function BlockLevelPick({
  pseudoLabel,
  handle,
  onHandleChange,
  candidates,
  onOpen,
}: BlockLevelPickProps) {
  const { t } = useTheme();
  const cardRef = useContext(DialogCardCtx);
  const resolved = candidates.status === "resolved";
  const list = resolved ? candidates.list : [];

  const blocks = groupByBlock(list);
  const { handleBlockKey, blockGroup, matched, blockStale, paramStale } =
    resolveHandle(blocks, handle, resolved);
  // The control dropdown has nothing to show without a confirmed block — hidden for
  // the pseudo pick and for a block-stale handle; shown (with its own removed-param
  // note) as soon as the block is confirmed, matched or not.
  const showControl = handle != null && resolved && !blockStale;

  // The single overall-best candidate — `recommendedBlock` walks `bestEnabled` per
  // block, so `recommended` is ALWAYS an enabled candidate (or `null` when every
  // block is fully disabled); it can still sit on a block that has OTHER, disabled
  // candidates (Scene's per-candidate scope), which is why `controlRow` still checks
  // `c.disabled` ahead of `rec` below — defensive, since the two conditions can never
  // both be true for the same candidate any more.
  const recommended = recommendedBlock(blocks);
  const recommendedBlockKey = recommended ? blockKeyOf(recommended) : null;

  // `groupHead` (not a bare `blockGroup[0]`) — the third spot that would otherwise
  // re-spell the same defensive "array might type as empty" guard `pickBlock` and
  // `blockRow` use below.
  const blockHead = blockGroup ? groupHead(blockGroup) : undefined;
  const blockArt = blockHead ? blockArtTile(blockHead.fenderId) : null;
  const blockTriggerLabel = blockTriggerLabelFor(
    handle,
    pseudoLabel,
    blockArt,
    resolved,
  );
  const controlTriggerLabel = controlTriggerLabelFor(handle, matched);

  // Two independent `usePickAnchor` instances — DESTRUCTURED (not kept as one
  // `blockPick`/`controlPick` object) because the hook returns refs
  // (`menuRef`/`triggerRef`) alongside plain state: `react-hooks/refs` taints every
  // property read off an un-destructured hook-result object once ANY of its fields is
  // a ref, so a bare `blockPick.anchor` reads as a ref access even though `anchor` is
  // plain `useState`. Destructuring gives each field its own precise binding, exactly
  // like every other `Pick`-family caller already does.
  const {
    open: blockOpen,
    anchor: blockAnchor,
    pos: blockPos,
    cardEl: blockCardEl,
    menuRef: blockMenuRef,
    triggerRef: blockTriggerRef,
    openMenu: openBlockMenu,
    close: closeBlockMenu,
  } = usePickAnchor(cardRef, {
    onOpen,
    // Grows AFTER it opens (the lazy fetch resolves a moment later) — same key
    // shape as the original combined picker's, so the menu re-clamps/re-flips once
    // real rows land.
    contentKey: `${candidates.status}:${String(blocks.length)}`,
  });
  const {
    open: ctrlOpen,
    anchor: ctrlAnchor,
    pos: ctrlPos,
    cardEl: ctrlCardEl,
    menuRef: ctrlMenuRef,
    triggerRef: ctrlTriggerRef,
    openMenu: openCtrlMenu,
    close: closeCtrlMenu,
  } = usePickAnchor(cardRef, { onOpen });

  const pickBlock = (group: BlockLevelCandidate[]) => {
    // `group` is one of `blocks`' entries — never empty (see `groupByBlock`), but
    // `groupHead` still spells it out as `T | undefined`: this tsconfig has no
    // `noUncheckedIndexedAccess`, so a bare `group[0]` types as always-defined.
    const first = groupHead(group);
    if (!first) return;
    const key = blockKeyOf(first);
    if (key === handleBlockKey) {
      // Re-selecting the block that already holds the stored handle: a click that
      // LOOKS like navigation must never silently change a stored handle (holds even
      // when the stored param itself is stale — the user must open the control
      // dropdown to fix that explicitly).
      closeBlockMenu();
      return;
    }
    // Selecting a DIFFERENT block auto-picks its best ENABLED candidate (fewest
    // clicks), via the SAME `bestEnabled` the "Recommended" tag reads. A block row
    // is only clickable when it has one (see `blockDisabled` below), so `best` is
    // always found here.
    const best = bestEnabled(group);
    if (!best) return;
    onHandleChange({
      groupId: best.groupId,
      nodeId: best.nodeId,
      parameterId: best.parameterId,
    });
    closeBlockMenu();
  };

  const blockRow = (group: BlockLevelCandidate[]) => {
    const first = groupHead(group);
    if (!first) return null;
    const key = blockKeyOf(first);
    const blockDisabled = group.every((c) => c.disabled === true);
    const disabledReason = blockDisabled
      ? group.find(isDisabledCandidate)?.disabledTitle
      : undefined;
    const rec = key === recommendedBlockKey;
    const art = blockArtTile(first.fenderId);
    return (
      <BlockPickRow
        key={key}
        pickKey={key}
        art={art}
        label={art.fullName ?? art.name}
        selected={key === handleBlockKey}
        disabled={blockDisabled}
        disabledTitle={disabledReason}
        onPick={() => {
          pickBlock(group);
        }}
        note={
          blockDisabled ? (
            <PickWarnNote>{disabledReason}</PickWarnNote>
          ) : rec ? (
            <Tag tone="good" uppercase>
              Recommended
            </Tag>
          ) : undefined
        }
      />
    );
  };

  const controlRow = (c: BlockLevelCandidate) => {
    // `c` iterates over `blockGroup` (`(blockGroup ?? []).map(controlRow)` below) —
    // the SAME array `matched` was found from, so a referential compare stands in
    // for the field-by-field one (`matched` already narrows to this block's own
    // `groupId`/`nodeId`, since `blockGroup` is the block holding the stored handle).
    const on = c === matched;
    const rec = c === recommended;
    const loud = c.paramClass !== "wet_mix";
    return (
      <BlockParamRow
        key={`${c.nodeId}:${c.parameterId}`}
        pickKey={`${c.nodeId}:${c.parameterId}`}
        paramLabel={paramLabel(c.parameterId)}
        selected={on}
        disabled={c.disabled}
        disabledTitle={c.disabled ? c.disabledTitle : undefined}
        onPick={() => {
          onHandleChange({
            groupId: c.groupId,
            nodeId: c.nodeId,
            parameterId: c.parameterId,
          });
          closeCtrlMenu();
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
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        gap: t.space2,
        width: "100%",
        minWidth: 0,
      }}
    >
      <PickTrigger
        triggerRef={blockTriggerRef}
        open={blockOpen}
        warn={blockStale}
        label={blockTriggerLabel}
        title="Choose this sound's leveling block"
        onClick={openBlockMenu}
      />

      {blockOpen && blockAnchor && blockCardEl && (
        <PickPortalMenu
          cardEl={blockCardEl}
          menuRef={blockMenuRef}
          left={blockPos ? blockPos.left : blockAnchor.left}
          top={blockPos ? blockPos.top : blockAnchor.below}
          visible={blockPos != null}
          minWidth={Math.max(blockAnchor.width, 280)}
          onClose={closeBlockMenu}
        >
          {pseudoLabel != null && (
            <div
              onClick={(e) => {
                e.stopPropagation();
                onHandleChange(null);
                closeBlockMenu();
              }}
              style={{
                display: "flex",
                alignItems: "center",
                gap: t.space5,
                padding: `${String(t.space3)}px ${String(t.space4)}px`,
                borderRadius: 5,
                cursor: "pointer",
                background: handle == null ? t.accentSoft : "transparent",
              }}
              onMouseEnter={(e) => {
                if (handle != null) e.currentTarget.style.background = t.hover;
              }}
              onMouseLeave={(e) => {
                if (handle != null)
                  e.currentTarget.style.background = "transparent";
              }}
            >
              <span
                style={{
                  fontFamily: t.mono,
                  fontSize: 11,
                  color: handle == null ? t.accentDeep : t.ink2,
                }}
              >
                {pseudoLabel} (default)
              </span>
              {handle == null && (
                <span style={{ marginLeft: "auto" }}>
                  <Icon
                    name="check"
                    size={13}
                    stroke={t.accentDeep}
                    strokeWidth={2}
                  />
                </span>
              )}
            </div>
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
          {blocks.map(blockRow)}
          {blockStale && (
            <div style={{ padding: t.space4 }}>
              <PickWarnNote>
                stored pick no longer offered — pick again
                {pseudoLabel ? " or use the default" : ""}.
              </PickWarnNote>
            </div>
          )}
        </PickPortalMenu>
      )}

      {showControl && (
        <>
          <PickTrigger
            triggerRef={ctrlTriggerRef}
            open={ctrlOpen}
            warn={paramStale}
            label={controlTriggerLabel}
            title="Choose this sound's leveling parameter"
            onClick={openCtrlMenu}
          />

          {ctrlOpen && ctrlAnchor && ctrlCardEl && (
            <PickPortalMenu
              cardEl={ctrlCardEl}
              menuRef={ctrlMenuRef}
              left={ctrlPos ? ctrlPos.left : ctrlAnchor.left}
              top={ctrlPos ? ctrlPos.top : ctrlAnchor.below}
              visible={ctrlPos != null}
              minWidth={Math.max(ctrlAnchor.width, 280)}
              onClose={closeCtrlMenu}
            >
              {(blockGroup ?? []).map(controlRow)}
              {paramStale && (
                <div style={{ padding: t.space4 }}>
                  <PickWarnNote>
                    stored pick no longer offered — pick a control below.
                  </PickWarnNote>
                </div>
              )}
            </PickPortalMenu>
          )}
        </>
      )}
    </div>
  );
}

export default BlockLevelPick;
