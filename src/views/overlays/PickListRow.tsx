// src/views/overlays/PickListRow.tsx — the row chrome behind `BlockParamRow`
// (`FlatLevelPick`'s candidate row): click + `stopPropagation` + disabled guard, the
// `title` while disabled, hover/selected background, the label + optional sub-line,
// and the trailing check icon. `BlockParamRow` is a thin named wrapper over it that
// supplies the param label; the `data-block-param-pick` hook attribute is what e2e
// specs select on.

import type { ReactNode } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { plainButton } from "../../theme/tokens";
import { Icon } from "../../ui/Icon";

export interface PickListRowProps {
  label: string;
  /** A "Recommended" tag, a warning note, or the shared/per-candidate disabled
   *  reason. Undefined renders no sub-line at all. */
  note?: ReactNode;
  selected: boolean;
  /** True when the row is inert — dimmed, default cursor, no click. */
  disabled?: boolean;
  /** Shown as the row's `title` while disabled. */
  disabledTitle?: string;
  onPick: () => void;
  /** Carried on the row's `data-block-param-pick` e2e hook attribute. */
  pickKey?: string;
}

export function PickListRow({
  label,
  note,
  selected,
  disabled,
  disabledTitle,
  onPick,
  pickKey,
}: PickListRowProps) {
  const { t } = useTheme();
  return (
    // aria-disabled (not native `disabled`) keeps the disabled row's reason tooltip.
    <button
      type="button"
      role="option"
      aria-selected={selected}
      aria-disabled={disabled ? true : undefined}
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
        padding: t.space4,
        borderRadius: 8,
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
        {/* Always rendered (even with no note) so the label line sits the same
            distance from the row edge whether or not a note follows. */}
        <div style={{ marginTop: t.space2 }}>{note}</div>
      </div>
      {selected && (
        <span style={{ flexShrink: 0 }}>
          <Icon name="check" size={15} stroke={t.accentDeep} strokeWidth={2} />
        </span>
      )}
    </button>
  );
}
