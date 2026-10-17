// src/views/copy/PlaceBlocks.tsx — Step 2 (look B): place blocks into each target, then
// save.
//
// One scrolling main area: the "Copy from" card (choose the block to copy on its strip),
// the section label "Copy into N presets" (with "N can't be saved yet" and a "Show only
// these" / "Show all" link while two or more targets break a limit), then one card per
// target — in two columns on a wide window. The Footer: Back, Undo and Redo (they never
// touch your Tone Master Pro) and "N of M presets changed" on the left; on the right the
// reason Save waits (when there is one, in place of the count, which leaves the room an
// 800-wide window needs), the "I've backed up with Pro Control" tick and "Save to your
// Tone Master Pro", greyed until a preset changed, nothing breaks a limit and the tick is
// on.

import { useCallback, useState, type Dispatch } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { sectionLabel } from "../../theme/tokens";
import { useWindowWidth } from "../../lib/useWindowWidth";
import { Button } from "../../ui/primitives";
import { BackupAckLabel } from "../../ui/BackupAckLabel";
import { Footer } from "../../ui/Footer";
import { Icon } from "../../ui/Icon";
import { SourceCard } from "./SourceCard";
import { TargetEditCard } from "./TargetEditCard";
import { COPY_TEXT, cantSaveYet, changedOf, copyInto } from "./copyText";
import type { EditMap } from "./copyModel";
import type { CopyPreset } from "./useCopyLibrary";
import type { CopyAction, OpenBlock, PickedBlock } from "./copyFlow";
import type { BlockedSlot } from "./validateBlockEdit";

/** From this window width (px) the target cards sit two to a row. */
const WIDE_WINDOW = 1200;

/** The derived Save gating (CopyView computes it once per edit). */
export interface SaveGate {
  changedCount: number;
  /** Targets whose staged edit breaks a firmware cap (CPU or a block-count rule) —
   *  they block the save until fixed. */
  blocked: BlockedSlot[];
  saveBlocked: boolean;
  /** Why Save waits (besides the backup tick), or null. */
  reason: string | null;
}

export interface PlaceBlocksProps {
  from: CopyPreset;
  targets: number[];
  edit: EditMap;
  pick: PickedBlock | null;
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
  pick,
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
  const wide = useWindowWidth() >= WIDE_WINDOW;
  const [onlyBlocked, setOnlyBlocked] = useState(false);

  const { changedCount, blocked, saveBlocked, reason } = gate;
  const reasonBySlot = new Map(blocked.map((b) => [b.slot, b.reason]));
  // The filter link shows while two or more targets break a limit.
  const several = blocked.length > 1;
  const showBlocked = onlyBlocked && several;
  const visible = showBlocked
    ? targets.filter((s) => reasonBySlot.has(s))
    : targets;

  const onPick = useCallback(
    (uid: string, model: string) => {
      dispatch({ type: "pickBlock", uid, model });
    },
    [dispatch],
  );
  const onCloseMenu = useCallback(() => {
    dispatch({ type: "closeMenu" });
  }, [dispatch]);

  const caption = {
    fontSize: t.fontCaption,
    lineHeight: `${String(t.lineCaption)}px`,
    color: t.textSecondary,
  } as const;

  return (
    <div
      style={{
        flex: 1,
        minHeight: 0,
        display: "flex",
        flexDirection: "column",
        background: t.window,
        fontFamily: t.fontSans,
        color: t.text,
      }}
    >
      <div style={{ flex: 1, minHeight: 0, overflowY: "auto" }}>
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            gap: t.sp12,
            padding: `${String(t.sp16)}px ${String(t.sp24)}px ${String(t.sp16 + t.sp4)}px`,
          }}
        >
          <SourceCard from={from} pick={pick} onPick={onPick} />

          <div
            style={{
              display: "flex",
              alignItems: "baseline",
              gap: t.sp12,
              flexWrap: "wrap",
            }}
          >
            <span style={sectionLabel(t)}>{copyInto(targets.length)}</span>
            {several && (
              <>
                <span style={caption}>{cantSaveYet(blocked.length)}</span>
                <Button
                  variant="link"
                  size="sm"
                  aria-pressed={showBlocked}
                  onClick={() => {
                    setOnlyBlocked((v) => !v);
                  }}
                >
                  {showBlocked ? COPY_TEXT.showAll : COPY_TEXT.showOnly}
                </Button>
              </>
            )}
          </div>

          <div
            style={{
              display: "grid",
              gridTemplateColumns: wide
                ? "minmax(0, 1fr) minmax(0, 1fr)"
                : "minmax(0, 1fr)",
              gap: t.sp12,
              alignItems: "start",
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
                  pick={pick}
                  blocked={reasonBySlot.get(slot) ?? null}
                  onTapBlock={(uid) => {
                    dispatch({ type: "tapBlock", slot, uid });
                  }}
                  onRemove={(uid) => {
                    dispatch({ type: "remove", slot, uid });
                  }}
                  onApply={(uid, mode, model) => {
                    dispatch({ type: "apply", slot, uid, mode, model });
                  }}
                  onCloseMenu={onCloseMenu}
                />
              );
            })}
          </div>
        </div>
      </div>

      <Footer
        left={
          <>
            <Button
              variant="secondary"
              size="lg"
              onClick={() => {
                dispatch({ type: "back" });
              }}
            >
              {COPY_TEXT.back}
            </Button>
            <Button
              variant="plain"
              icon="undo"
              aria-label={COPY_TEXT.undo}
              disabled={!canUndo}
              onClick={() => {
                dispatch({ type: "undo" });
              }}
            />
            <Button
              variant="plain"
              icon="redo"
              aria-label={COPY_TEXT.redo}
              disabled={!canRedo}
              onClick={() => {
                dispatch({ type: "redo" });
              }}
            />
            {reason == null && (
              <span style={{ ...caption, whiteSpace: "nowrap" }}>
                {changedOf(changedCount, targets.length)}
              </span>
            )}
          </>
        }
        right={
          <>
            {saveBlocked && reason != null && (
              <span
                style={{
                  ...caption,
                  display: "flex",
                  alignItems: "flex-start",
                  gap: t.sp6,
                  minWidth: 0,
                  maxWidth: 320,
                  color: t.text,
                }}
              >
                <span
                  style={{ display: "flex", paddingTop: 1.5, flexShrink: 0 }}
                >
                  <Icon
                    name="warn-tri"
                    size={13}
                    stroke={t.caution}
                    strokeWidth={2}
                  />
                </span>
                {reason}
              </span>
            )}
            <BackupAckLabel
              checked={backedUp}
              onChange={(value) => {
                dispatch({ type: "setBackedUp", value });
              }}
            />
            <Button
              variant="primary"
              size="lg"
              disabled={saveBlocked || !backedUp}
              onClick={onSave}
            >
              {COPY_TEXT.save}
            </Button>
          </>
        }
      />
    </div>
  );
}
