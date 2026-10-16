// src/views/level/SceneRow.tsx — one sound sub-row under an open preset, look B: the
// main sound, a scene or a footswitch.
//
// A 30 px compact Row with the grey `rowSelected` fill, indented 64 px under the preset's
// name. The WHOLE row is one role="checkbox" button named "Choose <sound> in <preset>"
// (unique on the page): a box, the sound's name (13/400) and, for a footswitch, a caption
// at the right ("footswitch 3"). A footswitch with no volume knob to level (BUG 1) is
// disabled: an empty grey box, the name in `textDisabled`, the caption saying why, and
// the click does nothing.

import { useTheme } from "../../theme/ThemeContext";
import { plainButton } from "../../theme/tokens";
import { CheckboxMark } from "../../ui/primitives";
import { Row } from "../../ui/List";

/** The sub-row's left inset: its box sits under the preset's slot column. */
const SUB_ROW_INDENT = 64;

export interface SceneRowProps {
  /** The sound's name ("Main sound", the scene's name, the footswitch's label). */
  name: string;
  /** The preset it belongs to — completes the accessible name. */
  preset: string;
  /** A short caption at the right: "footswitch 3", or why it can't be chosen. */
  caption?: string;
  selected: boolean;
  onToggle: () => void;
  /** Not selectable at all (BUG 1 — a footswitch with no volume knob to level): greyed
   *  out, the box renders empty, and the click is a no-op. `caption` is expected to
   *  carry the reason in this state. */
  disabled?: boolean;
}

export function SceneRow({
  name,
  preset,
  caption,
  selected,
  onToggle,
  disabled = false,
}: SceneRowProps) {
  const { t } = useTheme();
  return (
    <Row size="compact" selected style={{ padding: 0 }}>
      <button
        type="button"
        role="checkbox"
        aria-checked={!disabled && selected}
        aria-label={`Choose ${name} in ${preset}`}
        disabled={disabled}
        onClick={onToggle}
        style={plainButton({
          flex: 1,
          minWidth: 0,
          height: "100%",
          display: "flex",
          alignItems: "center",
          gap: t.sp8 + t.sp2,
          padding: `0 ${String(t.sp12)}px 0 ${String(SUB_ROW_INDENT)}px`,
          textAlign: "left",
          cursor: disabled ? "default" : "pointer",
        })}
      >
        <CheckboxMark checked={selected} disabled={disabled} />
        <span
          style={{
            flex: 1,
            minWidth: 0,
            fontSize: t.fontBody,
            fontWeight: t.weightRegular,
            color: disabled ? t.textDisabled : t.text,
            whiteSpace: "nowrap",
            overflow: "hidden",
            textOverflow: "ellipsis",
          }}
        >
          {name}
        </span>
        {caption != null && (
          <span
            style={{
              fontSize: t.fontCaption,
              color: t.textSecondary,
              whiteSpace: "nowrap",
              flexShrink: 0,
            }}
          >
            {caption}
          </span>
        )}
      </button>
    </Row>
  );
}
