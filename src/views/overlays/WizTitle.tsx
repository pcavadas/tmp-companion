// src/views/overlays/WizTitle.tsx — the serif body title each wizard stage opens with.

import type { CSSProperties, ReactNode } from "react";

import { useTheme } from "../../theme/ThemeContext";

// ---- serif body title (each stage's header) --------------------------------
export interface WizTitleProps {
  children: ReactNode;
  size?: number;
  style?: CSSProperties;
}

export function WizTitle({ children, size = 22, style }: WizTitleProps) {
  const { t } = useTheme();
  return (
    <div
      style={{
        fontFamily: t.serif,
        fontSize: size,
        color: t.ink,
        lineHeight: 1.14,
        letterSpacing: "-0.01em",
        ...style,
      }}
    >
      {children}
    </div>
  );
}
