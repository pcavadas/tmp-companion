// src/views/settings/TargetRow.tsx — one editable loudness-target row: drag
// handle · serif name (double-click to rename) · terracotta-filled slider track
// + white ringed knob · right-aligned mono value · ⋯ menu (Rename / Delete).
// Replaces the old read-only TargetSlider (handoff design_handoff_settings_targets:
// targets are now user-owned — create / rename / reorder / delete + draggable
// slider, with NO upper ceiling / clamp).

import { useRef, useState } from "react";
import type React from "react";

import { useTheme } from "../../theme/ThemeContext";
import { plainInput } from "../../theme/tokens";
import { Icon } from "../../ui/Icon";
import { MenuItem, MenuDivider } from "../../ui/primitives";
import { Menu } from "../../ui/Menu";

// Slider domain (handoff): v ∈ [−32 .. −16]. No upper clamp — any value in range
// is valid (the old ≈ −20.7 ceiling warning was removed).
const TMIN = -32;
const TMAX = -16;
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
  /** HTML5 DnD reorder: record this row as the drag source / drop target. */
  onGrab: () => void;
  onDropOn: () => void;
}

export function TargetRow({
  name,
  lufs,
  defaultEditing = false,
  onRename,
  onChange,
  onCommit,
  onDelete,
  onGrab,
  onDropOn,
}: TargetRowProps) {
  const { t } = useTheme();

  const [editing, setEditing] = useState(defaultEditing);
  const [draft, setDraft] = useState(name);
  const [menu, setMenu] = useState(false);
  const trackRef = useRef<HTMLDivElement | null>(null);

  // Keep the draft in sync with the row's name while not editing (e.g. after a
  // reorder reuses this row for a different target). Done via React's "adjust
  // state during render when a prop changes" pattern rather than an effect.
  const [prevName, setPrevName] = useState(name);
  if (name !== prevName) {
    setPrevName(name);
    if (!editing) setDraft(name);
  }

  function commit() {
    const n = draft.trim();
    if (n) onRename(n);
    else setDraft(name); // empty → revert
    setEditing(false);
  }
  function cancelEdit() {
    setDraft(name);
    setEditing(false);
  }

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
    onChange(valueAtClientX(e.clientX));
    const move = (ev: PointerEvent) => {
      onChange(valueAtClientX(ev.clientX));
    };
    const up = (ev: PointerEvent) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      onCommit(valueAtClientX(ev.clientX)); // persist once, on release
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
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
          <input
            autoFocus
            value={draft}
            onChange={(e) => {
              setDraft(e.target.value);
            }}
            onBlur={commit}
            onFocus={(e) => {
              e.currentTarget.select();
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") commit();
              if (e.key === "Escape") cancelEdit();
            }}
            style={plainInput(t, {
              width: "100%",
              boxSizing: "border-box",
              border: `0.5px solid ${t.accent}`,
              borderRadius: t.rBtn,
              padding: `${String(t.space2)}px ${String(t.space3)}px`,
              fontFamily: t.serif,
              fontSize: 15,
            })}
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
        <span
          onClick={() => {
            setMenu((o) => !o);
          }}
          title="More"
          style={{
            cursor: "pointer",
            display: "flex",
            color: t.faint,
            padding: t.space1,
            borderRadius: t.rMenuItem,
          }}
        >
          <Icon name="more" size={16} stroke="currentColor" />
        </span>
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
            <MenuDivider />
            <MenuItem label="Delete" onClick={closeMenuThen(onDelete)} danger />
          </Menu>
        )}
      </div>
    </div>
  );
}
