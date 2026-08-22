// src/views/overlays/PickTrigger.tsx — the bordered-pill trigger chrome shared,
// byte-for-byte, by `BlockLevelPick`'s BLOCK and CONTROL triggers (D2/Part C): a
// ref'd anchor wrapper, a warn icon (when `warn`), a truncating label, and a
// chevron that reflects open state. Only the MARKUP is unified here — each
// dropdown's own warn PREDICATE (`blockStale` / `paramStale`) stays with its
// owner and is passed in, same as `pickTriggerChrome.ts`'s shared border-color
// rule leaves every `Pick`-family warn predicate local. `BlockLevelPick` is this
// component's sole caller.

import type { MouseEvent, RefObject } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Icon } from "../../ui/Icon";
import { pickTriggerBorder } from "./pickTriggerChrome";

const TRIGGER_HEIGHT = 26;

export interface PickTriggerProps {
  triggerRef: RefObject<HTMLDivElement | null>;
  open: boolean;
  warn: boolean;
  label: string;
  title: string;
  onClick: (e: MouseEvent) => void;
}

export function PickTrigger({
  triggerRef,
  open,
  warn,
  label,
  title,
  onClick,
}: PickTriggerProps) {
  const { t } = useTheme();
  return (
    <div
      ref={triggerRef}
      style={{ position: "relative", width: "100%", minWidth: 0 }}
    >
      <div
        onClick={onClick}
        title={title}
        style={{
          display: "flex",
          alignItems: "center",
          gap: t.space3,
          height: TRIGGER_HEIGHT,
          padding: `0 ${String(t.space4)}px`,
          boxSizing: "border-box",
          border: pickTriggerBorder(t, { open, warn }),
          borderRadius: 6,
          background: t.bg,
          cursor: "pointer",
          whiteSpace: "nowrap",
          overflow: "hidden",
        }}
      >
        {warn && (
          <Icon
            name="warn-tri"
            size={12}
            stroke={t.sevWarn}
            strokeWidth={1.7}
          />
        )}
        <span
          style={{
            flex: 1,
            minWidth: 0,
            fontFamily: t.sans,
            fontSize: 11,
            color: warn ? t.sevWarn : t.ink2,
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
      </div>
    </div>
  );
}

export default PickTrigger;
