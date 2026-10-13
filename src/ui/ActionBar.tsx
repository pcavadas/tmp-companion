// src/ui/ActionBar.tsx — the ONE bottom-bar shell used across the app.
//
// A single bottom-bar style: min-height 60, padding
// 0 20px, a 0.5px hairline top rule on the `bgAlt` band, space-between. The Copy
// feature's two steps and the Presets-view selection footer all render their content
// inside this shell so there is exactly one bottom-bar style.

import type { ReactNode } from "react";
import { useTheme } from "../theme/ThemeContext";

export interface ActionBarProps {
  /** Left content (status / hint). */
  left: ReactNode;
  /** Right content (the primary action + any adjacent controls). */
  right: ReactNode;
}

export function ActionBar({ left, right }: ActionBarProps) {
  const { t } = useTheme();
  return (
    <div
      style={{
        flexShrink: 0,
        minHeight: 60,
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        gap: t.space7,
        padding: `0 ${String(t.space9)}px`,
        borderTop: `0.5px solid ${t.hairline}`,
        background: t.bgAlt,
        boxSizing: "border-box",
      }}
    >
      {left}
      {right}
    </div>
  );
}

export default ActionBar;
