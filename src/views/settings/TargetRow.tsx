// src/views/settings/TargetRow.tsx — one editable loudness-target row: drag
// handle · serif name (double-click to rename) · terracotta-filled slider track
// + white ringed knob · right-aligned mono value · ⋯ menu (Rename / Move up /
// Move down / Delete). Drag-reorder stays, but the menu is the click-only path.
// The slider captures its pointer: moves and the release land on the track, and a
// pointercancel (or a lost capture) ends the drag WITHOUT committing, restoring
// the value it started from.
// Replaces the old read-only TargetSlider (handoff design_handoff_settings_targets:
// targets are now user-owned — create / rename / reorder / delete + draggable
// slider, with NO upper ceiling / clamp).

import { useRef, useState } from "react";
import type React from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { Button, MenuItem, MenuDivider } from "../../ui/primitives";
import { Menu } from "../../ui/Menu";
import { InlineEdit } from "../../ui/InlineEdit";

// Slider domain (handoff): v ∈ [−29 .. −13]. No upper clamp — any value in range
// is valid (the old ≈ −20.7 ceiling warning was removed). PR2 re-baseline:
// +3.0103 (rounded to +3) from the mono-era [−32, −16] domain, so the same
// slider position is the same audible loudness under the new 2-ch BS.1770
// convention — convention-adjusted, pending hardware re-validation.
const TMIN = -29;
const TMAX = -13;
const TSPAN = TMAX - TMIN;

// Knob position %, clamped only to the renderable track (a stored value outside
// the domain still paints sanely at an edge).
function pos(v: number): number {
  return Math.min(100, Math.max(0, ((v - TMIN) / TSPAN) * 100));
}

interface TargetRowProps {
  name: string;
  lufs: number;
  /** Start in rename mode with the text pre-selected (a freshly added row). */
  defaultEditing?: boolean;
  /** Commit a non-empty new name (empty/whitespace is rejected upstream-safe). */
  onRename: (name: string) => void;
  /** Live value during a drag (updates the fill/readout; does NOT persist). */
  onChange: (lufs: number) => void;
  /** Final value on pointer-release (persist once). */
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
  const trackRef = useRef<HTMLDivElement | null>(null);
  // The in-progress slider drag: which pointer owns it and the value it started
  // from (restored on a cancel). null = no drag.
  const dragRef = useRef<{ pointerId: number; start: number } | null>(null);

  function valueAtClientX(clientX: number): number {
    const el = trackRef.current;
    if (!el) return lufs;
    const rect = el.getBoundingClientRect();
    if (rect.width <= 0) return lufs;
    const ratio = Math.max(0, Math.min(1, (clientX - rect.left) / rect.width));
    // 0.1 LUFS steps — the leveller's resolution.
    return Math.round((TMIN + ratio * TSPAN) * 10) / 10;
  }

  function onTrackDown(e: React.PointerEvent<HTMLDivElement>) {
    e.preventDefault();
    dragRef.current = { pointerId: e.pointerId, start: lufs };
    // Capture so the move/up/cancel events keep landing on the track even when
    // the pointer leaves it. Absent in jsdom, hence the guard.
    if ("setPointerCapture" in e.currentTarget)
      e.currentTarget.setPointerCapture(e.pointerId);
    onChange(valueAtClientX(e.clientX));
  }
  function onTrackMove(e: React.PointerEvent<HTMLDivElement>) {
    if (dragRef.current?.pointerId !== e.pointerId) return;
    onChange(valueAtClientX(e.clientX));
  }
  function onTrackUp(e: React.PointerEvent<HTMLDivElement>) {
    if (dragRef.current?.pointerId !== e.pointerId) return;
    dragRef.current = null;
    onCommit(valueAtClientX(e.clientX)); // persist once, on release
  }
  // pointercancel / lost capture: end the drag without committing, and put the
  // live value back where the drag started.
  function onTrackCancel(e: React.PointerEvent<HTMLDivElement>) {
    const drag = dragRef.current;
    if (drag?.pointerId !== e.pointerId) return;
    dragRef.current = null;
    onChange(drag.start);
  }

  const closeMenuThen = (fn: () => void) => () => {
    setMenu(false);
    fn();
  };

  const p = pos(lufs);

  return (
    <div
      draggable={!editing}
      onDragStart={onGrab}
      onDragOver={(e) => {
        e.preventDefault();
      }}
      onDrop={onDropOn}
      onDragEnd={onDragEnd}
      style={{
        display: "flex",
        alignItems: "center",
        gap: t.space5,
        padding: `${String(t.space5)}px 0`,
        borderBottom: `0.5px solid ${t.hairline}`,
      }}
    >
      <span
        title="Drag to reorder"
        style={{
          cursor: "grab",
          display: "flex",
          color: t.faint,
          flexShrink: 0,
        }}
      >
        <Icon name="grip" size={14} stroke="currentColor" />
      </span>

      <div style={{ width: 100, flexShrink: 0 }}>
        {editing ? (
          <InlineEdit
            initial={name}
            selectOnFocus
            ariaLabel="Target name"
            onCommit={(n) => {
              // empty → revert (the editor closes on the stored name)
              if (n) onRename(n);
              setEditing(false);
            }}
            onCancel={() => {
              setEditing(false);
            }}
            style={{
              width: "100%",
              boxSizing: "border-box",
              border: `0.5px solid ${t.accent}`,
              borderRadius: t.rBtn,
              padding: `${String(t.space2)}px ${String(t.space3)}px`,
              fontFamily: t.serif,
              fontSize: 15,
            }}
          />
        ) : (
          <span
            onDoubleClick={() => {
              setEditing(true);
            }}
            title="Double-click to rename"
            style={{
              display: "block",
              fontFamily: t.serif,
              fontSize: 15,
              color: t.ink,
              cursor: "text",
              whiteSpace: "nowrap",
              overflow: "hidden",
              textOverflow: "ellipsis",
            }}
          >
            {name}
          </span>
        )}
      </div>

      <div
        ref={trackRef}
        onPointerDown={onTrackDown}
        onPointerMove={onTrackMove}
        onPointerUp={onTrackUp}
        onPointerCancel={onTrackCancel}
        onLostPointerCapture={onTrackCancel}
        // Pointer-driven (the app is click-only — no key handling), but exposed to
        // VoiceOver as a slider carrying its value and range.
        role="slider"
        aria-label={`${name} target`}
        aria-valuemin={TMIN}
        aria-valuemax={TMAX}
        aria-valuenow={lufs}
        aria-valuetext={`${lufs.toFixed(1)} LUFS`}
        title="Drag to set level"
        style={{
          flex: 1,
          height: 5,
          background: t.track,
          borderRadius: t.rPill,
          position: "relative",
          cursor: "pointer",
          touchAction: "none",
        }}
      >
        <div
          style={{
            position: "absolute",
            left: 0,
            top: 0,
            bottom: 0,
            width: `${String(p)}%`,
            background: t.accent,
            borderRadius: t.rPill,
          }}
        />
        <div
          style={{
            position: "absolute",
            left: `${String(p)}%`,
            top: "50%",
            transform: "translate(-50%,-50%)",
            width: 13,
            height: 13,
            borderRadius: t.rPill,
            background: t.knob,
            boxShadow: `0 0 0 0.5px ${t.knobRing}, 0 1px 3px rgba(0,0,0,0.2)`,
          }}
        />
      </div>

      <span
        style={{
          width: 44,
          textAlign: "right",
          fontFamily: t.mono,
          fontSize: t.fsData,
          color: t.ink,
          flexShrink: 0,
          fontVariantNumeric: "tabular-nums",
        }}
      >
        {lufs.toFixed(1)}
      </span>

      <div style={{ position: "relative", display: "flex", flexShrink: 0 }}>
        <Button
          variant="bare"
          aria-label={`More actions for ${name}`}
          aria-haspopup="menu"
          aria-expanded={menu}
          onClick={() => {
            setMenu((o) => !o);
          }}
          title="More"
          style={{
            display: "flex",
            color: t.faint,
            padding: t.space1,
            borderRadius: t.rMenuItem,
          }}
        >
          <Icon name="more" size={16} stroke="currentColor" />
        </Button>
        {menu && (
          <Menu
            onClose={() => {
              setMenu(false);
            }}
            zIndex={9}
            minWidth={140}
          >
            <MenuItem
              label="Rename"
              onClick={closeMenuThen(() => {
                setEditing(true);
              })}
            />
            {onMoveUp && (
              <MenuItem label="Move up" onClick={closeMenuThen(onMoveUp)} />
            )}
            {onMoveDown && (
              <MenuItem label="Move down" onClick={closeMenuThen(onMoveDown)} />
            )}
            <MenuDivider />
            <MenuItem label="Delete" onClick={closeMenuThen(onDelete)} danger />
          </Menu>
        )}
      </div>
    </div>
  );
}
