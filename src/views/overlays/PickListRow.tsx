// src/views/overlays/PickListRow.tsx — the ONE option row of the Pick family (#314):
// `Pick`'s option rows, and `FlatLevelPick`'s pseudo "default" row and candidate rows.
// A real `<button role="option">` with `aria-selected`; click + `stopPropagation` + a
// disabled guard (`aria-disabled`, not native `disabled`, so the reason tooltip in
// `title` survives); hover/selected background.
//
// Two densities:
//   · "compact" — a mono value line with an optional right-aligned `sub` (and, with
//     `check`, a trailing check when selected): `Pick`'s rows, the pseudo row.
//   · "rich" — a sans label with a `note` sub-line under it ("Recommended" / a warning
//     / a disabled reason) and a trailing check when selected: the candidate rows.
// The e2e hooks are `data-pick-option` (`optionHook`) and `data-block-param-pick`
// (`pickKey`).

import type { ReactNode } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { plainButton } from "../../theme/tokens";
import { Icon } from "../../ui/Icon";

export interface PickListRowProps {
  label: string;
  selected: boolean;
  onPick: () => void;
  /** Default "compact". */
  density?: "compact" | "rich";
  /** compact only: a right-aligned secondary label. */
  sub?: string;
  /** compact only: show a trailing check when selected ("rich" always does). */
  check?: boolean;
  /** rich only: the sub-line under the label. Undefined renders an empty line so
   *  every rich row's label sits at the same height. */
  note?: ReactNode;
  /** True when the row is inert — dimmed, default cursor, no click. */
  disabled?: boolean;
  /** Shown as the row's `title` while disabled. */
  disabledTitle?: string;
  /** e2e hook: `data-pick-option`. */
  optionHook?: string;
  /** e2e hook: `data-block-param-pick`. */
  pickKey?: string;
}

export function PickListRow({
  label,
  selected,
  onPick,
  density = "compact",
  sub,
  check,
  note,
  disabled,
  disabledTitle,
  optionHook,
  pickKey,
}: PickListRowProps) {
  const { t } = useTheme();
  const rich = density === "rich";
  return (
    <button
      type="button"
      role="option"
      aria-selected={selected}
      aria-disabled={disabled ? true : undefined}
      data-pick-option={optionHook}
      data-block-param-pick={pickKey}
      onClick={(e) => {
        e.stopPropagation();
        if (disabled) return;
        onPick();
      }}
      title={disabled ? disabledTitle : undefined}
      style={{
        ...plainButton(),
        width: "100%",
        textAlign: "left",
        display: "flex",
        alignItems: "center",
        gap: t.space5,
        padding: rich
          ? t.space4
          : `${String(t.space3)}px ${String(t.space4)}px`,
        borderRadius: rich ? 8 : 5,
        cursor: disabled ? "default" : "pointer",
        opacity: disabled ? 0.55 : 1,
        background: selected ? t.accentSoft : "transparent",
      }}
      onMouseEnter={(e) => {
        if (!selected && !disabled) e.currentTarget.style.background = t.hover;
      }}
      onMouseLeave={(e) => {
        if (!selected) e.currentTarget.style.background = "transparent";
      }}
    >
      {rich ? (
        <div style={{ flex: 1, minWidth: 0 }}>
          <span
            style={{
              fontFamily: t.sans,
              fontSize: 13,
              fontWeight: 500,
              color: t.ink,
              whiteSpace: "nowrap",
            }}
          >
            {label}
          </span>
          <div style={{ marginTop: t.space2 }}>{note}</div>
        </div>
      ) : (
        <>
          <span
            style={{
              fontFamily: t.mono,
              fontSize: 11,
              color: selected ? t.accentDeep : t.ink2,
            }}
          >
            {label}
          </span>
          {sub && (
            <span
              style={{
                fontFamily: t.mono,
                fontSize: 9.5,
                color: t.faint,
                marginLeft: "auto",
              }}
            >
              {sub}
            </span>
          )}
        </>
      )}
      {selected && (rich || check) && (
        <span style={{ flexShrink: 0, marginLeft: "auto" }}>
          <Icon
            name="check"
            size={rich ? 15 : 13}
            stroke={t.accentDeep}
            strokeWidth={2}
          />
        </span>
      )}
    </button>
  );
}
