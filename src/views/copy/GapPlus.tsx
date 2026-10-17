// src/views/copy/GapPlus.tsx — the "+" in a gap of a Copy target strip (look B).
//
// Drawn by the signal-path strip in place of the wire between two blocks, and at each
// end of a row, only while a source block is chosen (`StripGraph.gap`, set by CopyPath).
// A 20 px circle with a dashed `action` edge, a white fill and a "+" in `action`; it keeps
// the wire's column shape (the 18 px label zone under it), so it sits on the wire line and
// the tiles keep their place. When placing the chosen block there would break a firmware
// limit it is disabled (a `textDisabled` edge) and the card says why under its strip.
// Its accessible name says where the block lands ("Add here in Scooped Verse, before
// Greenbox 8", "… at the end").

import { useTheme } from "../../theme/ThemeContext";
import { plainButton } from "../../theme/tokens";
import { Icon } from "../../ui/Icon";
import { STRIP_LBL } from "../signalChain/stripTypes";

/** The circle's size (px). */
const GAP_SIZE = 20;

export interface GapPlusProps {
  /** Where the block lands, as the button's accessible name. */
  label: string;
  /** Which side of its anchor block the gap places the block (e2e hook). */
  mode: "before" | "after";
  disabled: boolean;
  onPlace: () => void;
}

export function GapPlus({ label, mode, disabled, onPlace }: GapPlusProps) {
  const { t } = useTheme();
  const ink = disabled ? t.textDisabled : t.action;
  return (
    <div style={{ display: "flex", flexDirection: "column", flexShrink: 0 }}>
      <div
        style={{
          flex: 1,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        <button
          type="button"
          aria-label={label}
          data-gap={mode}
          disabled={disabled}
          onClick={onPlace}
          style={plainButton({
            width: GAP_SIZE,
            height: GAP_SIZE,
            boxSizing: "border-box",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            borderRadius: t.radiusPill,
            border: `1px dashed ${ink}`,
            background: t.surface,
            cursor: disabled ? "default" : "pointer",
          })}
        >
          <Icon name="plus" size={11} stroke={ink} strokeWidth={2.6} />
        </button>
      </div>
      <div style={{ height: STRIP_LBL }} />
    </div>
  );
}
