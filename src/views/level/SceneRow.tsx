// src/views/level/SceneRow.tsx — one scene sub-row (Base or a footswitch scene).
//
// Revealed under a PresetRow when its caret is expanded. Grid `34px 1fr`, height 38,
// indented to align under the preset name. The whole row toggles that one scene key.
//   • Base child: tag BASE (muted), name "Base Preset", sub "main preset sound".
//   • FS child:   tag FS{n} (terracotta), name = scene name, sub "footswitch scene".

import { useTheme } from "../../theme/ThemeContext";
import { Checkbox } from "../../ui/primitives";
import { Tag } from "../../ui/Tag";

export interface SceneRowProps {
  kind: "base" | "fs";
  /** Tag chip text: "BASE" | `FS${n}` | "—". */
  tag: string;
  name: string;
  sub: string;
  selected: boolean;
  onToggle: () => void;
  /** Not selectable at all (BUG 1 — a footswitch with no level control to adjust):
   *  greyed out, the checkbox renders inert, and the click is a no-op. `sub` is
   *  expected to carry the short reason in this state (the caller swaps its text). */
  disabled?: boolean;
  /** A full-sentence reason, shown as the row's title tooltip when `disabled`. */
  disabledTitle?: string;
}

export function SceneRow({
  kind,
  tag,
  name,
  sub,
  selected,
  onToggle,
  disabled,
  disabledTitle,
}: SceneRowProps) {
  const { t } = useTheme();
  const isBase = kind === "base";

  return (
    <div
      onClick={disabled ? undefined : onToggle}
      title={disabled ? disabledTitle : undefined}
      style={{
        display: "grid",
        gridTemplateColumns: "34px 1fr",
        alignItems: "center",
        height: 38,
        padding: `0 ${String(t.space8)}px 0 58px`,
        borderBottom: `0.5px solid ${t.hairline}`,
        background: !disabled && selected ? t.rowSel : t.bgAlt,
        opacity: disabled ? 0.55 : 1,
        cursor: disabled ? "default" : "pointer",
      }}
    >
      <div style={{ display: "flex", alignItems: "center", height: "100%" }}>
        <Checkbox checked={selected} disabled={disabled} />
      </div>
      <span
        style={{
          display: "flex",
          alignItems: "center",
          gap: t.space4,
          minWidth: 0,
        }}
      >
        <Tag tone={isBase ? "neutral" : "accent"}>{tag}</Tag>
        <span
          style={{
            fontFamily: t.serif,
            fontSize: t.fsName2,
            color: t.ink2,
            whiteSpace: "nowrap",
            overflow: "hidden",
            textOverflow: "ellipsis",
          }}
        >
          {name}
        </span>
        <span style={{ flex: 1 }} />
        <span
          style={{
            fontFamily: t.mono,
            fontSize: t.fsMicro,
            letterSpacing: "0.03em",
            color: t.faint,
            whiteSpace: "nowrap",
            flexShrink: 0,
          }}
        >
          {sub}
        </span>
      </span>
    </div>
  );
}

export default SceneRow;
