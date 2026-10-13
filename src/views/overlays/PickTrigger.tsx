// src/views/overlays/PickTrigger.tsx — the ONE trigger of the Pick family (#314),
// shared by `Pick` and `FlatLevelPick`: a ref'd anchor wrapper (the menu measures
// against it, and the inline fallback menu renders inside it as `children`), a
// bordered-pill button carrying the listbox ARIA, a warn icon (when `warn`), a
// truncating label, and a chevron that reflects open state.
//
// Only the MARKUP is shared. Each picker's WARN PREDICATE stays with its owner and is
// passed in (`Pick`'s "stored value not in options", `FlatLevelPick`'s "stored handle
// not in the fetched candidates") — they operate on different shapes, and unifying
// them would hide real per-picker meaning behind a shared name. The border is the
// one 3-way rule: open beats warn beats the resting hairline.

import type { MouseEvent, ReactNode, RefObject } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { plainButton } from "../../theme/tokens";
import { Icon } from "../../ui/Icon";

const TRIGGER_HEIGHT = 26;

export interface PickTriggerProps {
  triggerRef: RefObject<HTMLDivElement | null>;
  open: boolean;
  warn: boolean;
  label: string;
  title?: string;
  onClick: (e: MouseEvent<HTMLButtonElement>) => void;
  /** Label face: "mono" (a value chip — `Pick`) or "sans" (a control name —
   *  `FlatLevelPick`). Default "mono". */
  face?: "mono" | "sans";
  /** Render the label faint (a per-scene picker FOLLOWING the apply-to-all value). */
  muted?: boolean;
  /** Fill the parent's width. Default true. */
  grow?: boolean;
  /** e2e hook: the trigger's `data-pick` attribute. */
  tid?: string;
  /** Rendered inside the anchor wrapper after the button (the inline fallback menu). */
  children?: ReactNode;
}

export function PickTrigger({
  triggerRef,
  open,
  warn,
  label,
  title,
  onClick,
  face = "mono",
  muted,
  grow = true,
  tid,
  children,
}: PickTriggerProps) {
  const { t } = useTheme();
  const mono = face === "mono";
  return (
    <div
      ref={triggerRef}
      style={{
        position: "relative",
        width: grow ? "100%" : undefined,
        minWidth: grow ? 0 : undefined,
      }}
    >
      <button
        type="button"
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={onClick}
        title={title}
        data-pick={tid}
        style={{
          ...plainButton(),
          width: "100%",
          textAlign: "left",
          display: "flex",
          alignItems: "center",
          gap: t.space3,
          height: TRIGGER_HEIGHT,
          padding: `0 ${String(t.space3)}px 0 ${String(t.space4)}px`,
          boxSizing: "border-box",
          border: `0.5px solid ${open ? t.accent : warn ? t.sevWarn : t.hairlineStrong}`,
          borderRadius: 6,
          background: t.bg,
          whiteSpace: "nowrap",
          overflow: "hidden",
        }}
      >
        {warn && (
          <Icon
            name="warn-tri"
            size={11}
            stroke={t.sevWarn}
            strokeWidth={1.7}
          />
        )}
        <span
          style={{
            flex: 1,
            minWidth: 0,
            fontFamily: mono ? t.mono : t.sans,
            fontSize: mono ? 10.5 : 11,
            color: warn ? t.sevWarn : muted ? t.faint : mono ? t.ink : t.ink2,
            overflow: "hidden",
            textOverflow: "ellipsis",
          }}
        >
          {label}
        </span>
        <Icon
          name="chev-down"
          size={11}
          stroke={open ? t.accentDeep : t.faint}
        />
      </button>
      {children}
    </div>
  );
}
