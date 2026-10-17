// src/ui/Select.tsx — the ONE dropdown (look B). It replaced the old Pick family, deleted
// once the last screen that used it (Doctor) moved to look B.
//
// A button showing the current value and an up/down chevron; a click opens an anchored
// list in the `Menu` chrome (portaled to <body>, placed from the trigger's own rect, left
// edges flush, flipped above near the window bottom, re-placed when its content lands
// late). Sizes `sm` 22 (inside rows) and `md` 28 (page level); edge `controlBorder`.
//
// It ALWAYS shows the value that will be used (danger.md: a dropdown must display the
// value that will be used):
//   - an offered value shows its option's label;
//   - a value the options no longer contain shows the stored value itself
//     (`staleLabel`, else the raw value) — NEVER `options[0]`; the caller's row note says
//     why (`resolveSelectValue` tells it);
//   - no value (`""`) shows the placeholder in secondary text, in the `attention` look
//     (`cautionLine` edge + a 2 px `cautionFill` ring) — a choice is needed first;
//   - a "for all" select whose rows differ shows "Mixed" (never an option in its list).
// The accessible name says it all: "<label>: <value>" ("Target for Plexi Crunch, Rhythm:
// Rhythm"), `aria-haspopup="listbox"`, `aria-expanded`.
//
// Click-only: no key handling (the trigger and the options are real buttons, so Tab,
// Space and Enter work natively).

import { useState, type CSSProperties, type MouseEvent } from "react";

import { useTheme } from "../theme/ThemeContext";
import { plainButton } from "../theme/tokens";
import { Icon } from "./Icon";
import { Menu } from "./Menu";
import { Spinner } from "./Spinner";
import { resolveSelectValue, type SelectOption } from "./selectValue";

export type { SelectOption } from "./selectValue";

export interface SelectProps {
  /** The value that will be used; `""` = nothing chosen yet. */
  value: string;
  options: readonly SelectOption[];
  onChange: (value: string) => void;
  /** What it chooses, for what ("Target for Plexi Crunch, Rhythm"). The accessible
   *  name is "<label>: <shown value>"; it also names the open list. */
  label: string;
  /** `sm` 22 high (inside rows), `md` 28 (page level). Default `md`. */
  size?: "sm" | "md";
  /** Trigger width (px or a CSS length). Default: as wide as its value. */
  width?: number | string;
  /** Shown, in secondary text, when nothing is chosen. Default "Choose". */
  placeholder?: string;
  /** The "a choice is needed before the main action" look. Defaults to on exactly
   *  when nothing is chosen (and the select is usable). */
  attention?: boolean;
  /** A "for all" select over rows that differ: shows "Mixed"; no option is ticked. */
  mixed?: boolean;
  /** How to name a stored value the options no longer contain (default: the value). */
  staleLabel?: string;
  disabled?: boolean;
  /** Still reading what it will use: a spinner in place of the value, not clickable. */
  busy?: boolean;
  /** The list's options are still being read: the open list says `loadingText`. */
  loading?: boolean;
  /** Default "Reading…". */
  loadingText?: string;
  /** The open list when there is nothing to choose. Default "Nothing to choose". */
  emptyText?: string;
  /** A line under the options in the open list, in secondary text — a read that
   *  failed while the default stays usable ("Couldn't read this preset's knobs."). */
  listNote?: string;
  /** Open the list from outside (a footer link that points at this choice): each NEW
   *  non-zero value opens it once. 0 or an unchanged value does nothing. The caller
   *  fires its own lazy read first; `onOpen` runs only for a click. */
  openRequest?: number;
  /** Fired each time the list opens (a lazy per-preset read). Idempotent. */
  onOpen?: () => void;
  /** Fixed list width (px). Default: at least the trigger's width. */
  menuWidth?: number;
  /** e2e hook: `data-select` on the trigger, `data-select-option="<tid>:<value>"` on
   *  each option. */
  tid?: string;
}

/** Above dialogs (50) and toasts (60): a Select inside a Dialog opens over it. */
const LIST_Z = 70;

export function Select({
  value,
  options,
  onChange,
  label,
  size = "md",
  width,
  placeholder = "Choose",
  attention,
  mixed = false,
  staleLabel,
  disabled = false,
  busy = false,
  loading = false,
  loadingText = "Reading…",
  emptyText = "Nothing to choose",
  listNote,
  openRequest = 0,
  onOpen,
  menuWidth,
  tid,
}: SelectProps) {
  const { t } = useTheme();
  const [open, setOpen] = useState(false);
  // The trigger's width at open (measured in the click handler, never in render), so
  // the list is at least as wide as the dropdown it hangs from.
  const [anchorW, setAnchorW] = useState(0);
  // A new outside request opens the list (adjusted during render, never in an effect).
  const [seenRequest, setSeenRequest] = useState(openRequest);
  if (openRequest !== seenRequest) {
    setSeenRequest(openRequest);
    if (openRequest !== 0) setOpen(true);
  }

  const sm = size === "sm";
  const inert = disabled || busy;
  // A list never stays open over a select that just turned busy or disabled.
  const listOpen = open && !inert;
  const resolution = resolveSelectValue(value, options);
  const empty = !mixed && resolution.kind === "empty";
  const attn = !inert && (attention ?? empty);

  const shown = mixed
    ? "Mixed"
    : resolution.kind === "option"
      ? resolution.option.label
      : resolution.kind === "stale"
        ? (staleLabel ?? value)
        : placeholder;
  const spoken = busy
    ? "reading"
    : mixed
      ? "Mixed"
      : resolution.kind === "empty"
        ? "not chosen"
        : shown;

  const toggle = (e: MouseEvent<HTMLButtonElement>) => {
    // A Select can sit inside a clickable surface; its own click is its own.
    e.stopPropagation();
    if (listOpen) {
      setOpen(false);
      return;
    }
    setAnchorW(e.currentTarget.getBoundingClientRect().width);
    setOpen(true);
    onOpen?.();
  };

  const pick = (e: MouseEvent<HTMLButtonElement>, o: SelectOption) => {
    e.stopPropagation();
    if (o.disabled) return;
    setOpen(false);
    onChange(o.value);
  };

  const infoRow: CSSProperties = {
    display: "flex",
    alignItems: "center",
    height: t.control,
    padding: `0 ${String(t.sp8)}px`,
    fontSize: t.fontBody,
    color: t.textSecondary,
    whiteSpace: "nowrap",
  };

  return (
    <span
      style={{
        position: "relative",
        display: "inline-flex",
        width,
        minWidth: 0,
        flexShrink: width == null ? 0 : undefined,
      }}
    >
      <button
        type="button"
        aria-haspopup="listbox"
        aria-expanded={listOpen}
        aria-label={`${label}: ${spoken}`}
        aria-busy={busy ? true : undefined}
        disabled={inert}
        data-select={tid}
        data-attention={attn ? "" : undefined}
        onClick={toggle}
        style={plainButton({
          width: "100%",
          height: sm ? t.controlSmall : t.control,
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: t.sp6,
          padding: `0 ${String(sm ? t.sp6 : t.sp8)}px`,
          border: `1px solid ${
            attn ? t.cautionLine : inert ? t.textDisabled : t.controlBorder
          }`,
          boxShadow: attn ? `0 0 0 2px ${t.cautionFill}` : undefined,
          borderRadius: t.radiusControl,
          background: t.surface,
          fontFamily: t.fontSans,
          fontSize: sm ? t.fontCaption : t.fontBody,
          color: inert ? t.textDisabled : t.text,
          whiteSpace: "nowrap",
          overflow: "hidden",
          cursor: inert ? "default" : "pointer",
        })}
      >
        <span
          style={{
            flex: 1,
            minWidth: 0,
            textAlign: "left",
            overflow: "hidden",
            textOverflow: "ellipsis",
            color: inert ? t.textDisabled : empty ? t.textSecondary : t.text,
          }}
        >
          {busy ? null : shown}
        </span>
        {busy ? (
          <Spinner size={12} stroke={t.textSecondary} strokeWidth={2.2} />
        ) : (
          <Icon
            name="chev-updown"
            size={sm ? 12 : 13}
            stroke={inert ? t.textDisabled : t.textSecondary}
            strokeWidth={2}
          />
        )}
      </button>
      {listOpen && (
        <Menu
          role="listbox"
          label={label}
          align="start"
          zIndex={LIST_Z}
          width={menuWidth}
          minWidth={Math.max(anchorW, 120)}
          contentKey={`${String(loading)}:${String(options.length)}:${listNote ?? ""}`}
          onClose={() => {
            setOpen(false);
          }}
        >
          {loading ? (
            <div style={infoRow}>{loadingText}</div>
          ) : options.length === 0 ? (
            <div style={infoRow}>{emptyText}</div>
          ) : (
            options.map((o) => {
              const selected = !mixed && o.value === value;
              const off = o.disabled ?? false;
              return (
                <button
                  key={o.value}
                  type="button"
                  role="option"
                  aria-selected={selected}
                  aria-disabled={off ? true : undefined}
                  data-select-option={tid ? `${tid}:${o.value}` : undefined}
                  onClick={(e) => {
                    pick(e, o);
                  }}
                  onMouseEnter={(e) => {
                    if (!off) e.currentTarget.style.background = t.rowHover;
                  }}
                  onMouseLeave={(e) => {
                    e.currentTarget.style.background = "transparent";
                  }}
                  style={plainButton({
                    width: "100%",
                    height: t.control,
                    display: "flex",
                    alignItems: "center",
                    gap: t.sp6,
                    padding: `0 ${String(t.sp8)}px`,
                    borderRadius: t.radiusSmall,
                    fontFamily: t.fontSans,
                    fontSize: t.fontBody,
                    color: off ? t.textDisabled : t.text,
                    textAlign: "left",
                    whiteSpace: "nowrap",
                    cursor: off ? "default" : "pointer",
                  })}
                >
                  <span style={{ width: 12, flexShrink: 0, display: "flex" }}>
                    {selected && (
                      <Icon
                        name="check"
                        size={12}
                        stroke={t.text}
                        strokeWidth={2.4}
                      />
                    )}
                  </span>
                  <span
                    style={{
                      flex: 1,
                      minWidth: 0,
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                    }}
                  >
                    {o.label}
                    {o.note && (
                      <span
                        style={{ marginLeft: t.sp8, color: t.textSecondary }}
                      >
                        {o.note}
                      </span>
                    )}
                  </span>
                </button>
              );
            })
          )}
          {!loading && listNote != null && (
            <div style={infoRow}>{listNote}</div>
          )}
        </Menu>
      )}
    </span>
  );
}
