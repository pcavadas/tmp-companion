// src/ui/Steps.tsx — the flow breadcrumb at the right of a page title (look B):
// "Set up › Level › Summary" (Doctor: "Instrument › Check › Results", or "Check ›
// Results" when its Set up is skipped). 12 px; the current step in `text`/600 with
// `aria-current="step"`, the others in secondary text. Not clickable.

import { Fragment } from "react";

import { useTheme } from "../theme/ThemeContext";

export interface StepsProps {
  steps: readonly string[];
  /** 0-based index of the current step. */
  current: number;
}

export function Steps({ steps, current }: StepsProps) {
  const { t } = useTheme();
  return (
    <ol
      aria-label="Steps"
      style={{
        display: "flex",
        gap: t.sp6,
        listStyle: "none",
        margin: 0,
        padding: 0,
        fontFamily: t.fontSans,
        fontSize: t.fontCaption,
        lineHeight: `${String(t.lineCaption)}px`,
        whiteSpace: "nowrap",
      }}
    >
      {steps.map((step, i) => (
        <Fragment key={step}>
          {i > 0 && (
            <li aria-hidden style={{ color: t.textDisabled }}>
              ›
            </li>
          )}
          <li
            aria-current={i === current ? "step" : undefined}
            style={{
              color: i === current ? t.text : t.textSecondary,
              fontWeight: i === current ? t.weightSemibold : t.weightRegular,
            }}
          >
            {step}
          </li>
        </Fragment>
      ))}
    </ol>
  );
}
