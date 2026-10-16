// src/views/settings/TargetRow.tsx — one loudness-target row (look B), 46 high: a drag
// grip, the name (13/500 in a 90 px column; double-click or ⋯ Rename edits it in
// place), the DS `Slider` ("Quieter" … "Louder", the value as "−23.0 dB"), and a ⋯
// menu (Rename / Move up / Move down / Delete target). Drag-reorder stays, but the menu
// is the click-only path. The slider captures its pointer: a pointercancel (or a lost
// capture) ends the drag WITHOUT committing and restores the value it started from; a
// release commits once.

import { useState } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { InlineEdit } from "../../ui/InlineEdit";
import { Row } from "../../ui/List";
import { MoreMenu } from "../../ui/MoreMenu";
import { MenuDivider, MenuItem, Slider } from "../../ui/primitives";
import { fmtLufs } from "../../lib/format";

// Slider domain: v ∈ [−29 .. −13], with NO upper clamp — any value in range is valid
// (the old ≈ −20.7 ceiling warning was removed). PR2 re-baseline: +3.0103 (rounded to
// +3) from the mono-era [−32, −16] domain, so the same slider position is the same
// audible loudness under the 2-ch BS.1770 convention — convention-adjusted, pending
// hardware re-validation.
const TMIN = -29;
const TMAX = -13;

/** A target row is taller than a list row: it holds the slider's 18 px thumb. */
const TARGET_ROW_H = 46;
/** The name column, so the sliders line up down the list. */
const NAME_W = 90;

export interface TargetRowProps {
  name: string;
  lufs: number;
  /** Start in rename mode with the text pre-selected (a freshly added row). */
  defaultEditing?: boolean;
  /** Commit a non-empty new name (an empty one just closes the editor). */
  onRename: (name: string) => void;
  /** Live value during a drag (moves the fill and the value; does NOT persist). */
  onChange: (lufs: number) => void;
  /** Final value on release (persist once). */
  onCommit: (lufs: number) => void;
  onDelete: () => void;
  /** Click-only reorder; undefined at the list's edge (the item is hidden). */
  onMoveUp?: () => void;
  onMoveDown?: () => void;
  /** HTML5 DnD reorder: record this row as the drag source / drop target. */
  onGrab: () => void;
  onDropOn: () => void;
  /** A drag ended (dropped or abandoned) — clear the drag source. */
  onDragEnd: () => void;
}

export function TargetRow({
  name,
  lufs,
  defaultEditing = false,
  onRename,
  onChange,
  onCommit,
  onDelete,
  onMoveUp,
  onMoveDown,
  onGrab,
  onDropOn,
  onDragEnd,
}: TargetRowProps) {
  const { t } = useTheme();
  const [editing, setEditing] = useState(defaultEditing);
  const [menu, setMenu] = useState(false);
  const pick = (fn: () => void) => () => {
    setMenu(false);
    fn();
  };

  return (
    <Row
      draggable={!editing}
      onDragStart={onGrab}
      onDragOver={(e) => {
        e.preventDefault();
      }}
      onDrop={onDropOn}
      onDragEnd={onDragEnd}
      selected={menu}
      style={{ height: TARGET_ROW_H - 1 }}
    >
      {/* The drag handle. Not a button: dragging is its only use, and the ⋯ menu is
          the click path for the same moves. */}
      <span
        role="img"
        aria-label={`Move ${name}`}
        style={{
          width: 22,
          height: 22,
          flexShrink: 0,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          cursor: "grab",
        }}
      >
        <Icon name="grip" size={12} stroke={t.textSecondary} />
      </span>

      <span style={{ width: NAME_W, flexShrink: 0, minWidth: 0 }}>
        {editing ? (
          <InlineEdit
            initial={name}
            selectOnFocus
            ariaLabel="Target name"
            onCommit={(n) => {
              // Empty → the editor closes on the stored name.
              if (n) onRename(n);
              setEditing(false);
            }}
            onCancel={() => {
              setEditing(false);
            }}
            style={{
              width: "100%",
              height: t.controlSmall,
              boxSizing: "border-box",
              padding: `0 ${String(t.sp6)}px`,
              border: `1px solid ${t.controlBorder}`,
              borderRadius: t.radiusControl,
              background: t.surface,
              fontFamily: t.fontSans,
              fontSize: t.fontBody,
              fontWeight: t.weightMedium,
            }}
          />
        ) : (
          <span
            onDoubleClick={() => {
              setEditing(true);
            }}
            style={{
              display: "block",
              fontWeight: t.weightMedium,
              whiteSpace: "nowrap",
              overflow: "hidden",
              textOverflow: "ellipsis",
            }}
          >
            {name}
          </span>
        )}
      </span>

      <Slider
        value={lufs}
        min={TMIN}
        max={TMAX}
        label={`${name} target`}
        valueText={`${fmtLufs(lufs)} dB`}
        onChange={onChange}
        onCommit={onCommit}
      />

      <span style={{ flex: 1 }} />

      <MoreMenu label={`More for ${name}`} open={menu} onOpenChange={setMenu}>
        <MenuItem
          label="Rename"
          onClick={pick(() => {
            setEditing(true);
          })}
        />
        {onMoveUp && <MenuItem label="Move up" onClick={pick(onMoveUp)} />}
        {onMoveDown && (
          <MenuItem label="Move down" onClick={pick(onMoveDown)} />
        )}
        <MenuDivider />
        <MenuItem label="Delete target" danger onClick={pick(onDelete)} />
      </MoreMenu>
    </Row>
  );
}
