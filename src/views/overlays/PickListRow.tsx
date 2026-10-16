// src/views/overlays/PickListRow.tsx — `Pick`'s option row (#314). A real
// `<button role="option">` with `aria-selected`; click + `stopPropagation`;
// hover/selected background. A mono value line with an optional right-aligned `sub`.
// The e2e hook is `data-pick-option` (`optionHook`).

import { useTheme } from "../../theme/ThemeContext";
import { plainButton } from "../../theme/tokens";

export interface PickListRowProps {
  label: string;
  selected: boolean;
  onPick: () => void;
  /** A right-aligned secondary label. */
  sub?: string;
  /** e2e hook: `data-pick-option`. */
  optionHook?: string;
}

export function PickListRow({
  label,
  selected,
  onPick,
  sub,
  optionHook,
}: PickListRowProps) {
  const { t } = useTheme();
  return (
    <button
      type="button"
      role="option"
      aria-selected={selected}
      data-pick-option={optionHook}
      onClick={(e) => {
        e.stopPropagation();
        onPick();
      }}
      style={{
        ...plainButton(),
        width: "100%",
        textAlign: "left",
        display: "flex",
        alignItems: "center",
        gap: t.space5,
        padding: `${String(t.space3)}px ${String(t.space4)}px`,
        borderRadius: 5,
        cursor: "pointer",
        background: selected ? t.accentSoft : "transparent",
      }}
      onMouseEnter={(e) => {
        if (!selected) e.currentTarget.style.background = t.hover;
      }}
      onMouseLeave={(e) => {
        if (!selected) e.currentTarget.style.background = "transparent";
      }}
    >
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
    </button>
  );
}
