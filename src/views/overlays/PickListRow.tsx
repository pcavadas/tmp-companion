// src/views/overlays/PickListRow.tsx — the row chrome shared, byte-for-byte, by
// `BlockPickRow` (the BLOCK dropdown's row) and `BlockParamRow` (the CONTROL
// dropdown's row) — `BlockLevelPick`'s two-dropdown picker (D2/Part C): click +
// `stopPropagation` + disabled guard, the `title` while disabled, hover/selected
// background, and the trailing check icon. The two callers differ only in an
// optional LEADING slot (a BlockArt tile — block rows only), the label's font
// treatment (serif/14 for a block's full name vs sans/13/500 for a param label),
// and their own e2e hook attribute (`data-block-pick` vs `data-block-param-pick`)
// — kept as two thin named wrappers rather than one generic row so each caller's
// prop surface and hook attribute stay exactly what a spec already depends on.

import type { ReactNode } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";

export interface PickListRowProps {
  /** Rendered before the label (a BlockArt tile). Omitted entirely for a row with
   *  no leading visual — the label then sits flush left. */
  leading?: ReactNode;
  label: string;
  /** "block" ⇒ the serif/14 full-name treatment (`BlockPickRow`); "param" ⇒ the
   *  smaller sans/13/500 treatment (`BlockParamRow`). */
  labelEmphasis: "block" | "param";
  /** A "Recommended" tag, a warning note, or the shared/per-candidate disabled
   *  reason. Undefined renders no sub-line at all. */
  note?: ReactNode;
  selected: boolean;
  /** True when the row is inert — dimmed, default cursor, no click. */
  disabled?: boolean;
  /** Shown as the row's `title` while disabled. */
  disabledTitle?: string;
  onPick: () => void;
  /** Selects WHICH of the two `data-*` hook attributes below actually carries
   *  `pickKey` (the other renders `undefined`, so React omits it) — each wrapper
   *  keeps ITS OWN attribute name rather than a shared generic one an existing
   *  spec would otherwise stop finding. */
  dataAttr: "data-block-pick" | "data-block-param-pick";
  pickKey?: string;
}

export function PickListRow({
  leading,
  label,
  labelEmphasis,
  note,
  selected,
  disabled,
  disabledTitle,
  onPick,
  dataAttr,
  pickKey,
}: PickListRowProps) {
  const { t } = useTheme();
  return (
    <div
      data-block-pick={dataAttr === "data-block-pick" ? pickKey : undefined}
      data-block-param-pick={
        dataAttr === "data-block-param-pick" ? pickKey : undefined
      }
      onClick={(e) => {
        e.stopPropagation();
        if (disabled) return;
        onPick();
      }}
      title={disabled ? disabledTitle : undefined}
      style={{
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
      {leading}
      <div style={{ flex: 1, minWidth: 0 }}>
        <span
          style={
            labelEmphasis === "block"
              ? {
                  fontFamily: t.serif,
                  fontSize: 14,
                  color: t.ink,
                  whiteSpace: "nowrap",
                }
              : {
                  fontFamily: t.sans,
                  fontSize: 13,
                  fontWeight: 500,
                  color: t.ink,
                  whiteSpace: "nowrap",
                }
          }
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
    </div>
  );
}

export default PickListRow;
