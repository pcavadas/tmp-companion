// src/ui/PageState.tsx — the whole-content message for device and app states (look B):
// Connecting, Not connected, Pro Control is using it, Firmware not tested, Something
// went wrong, Couldn't read your presets.
//
// Centred in its pane: a 36 px outline icon tinted by tone (`neutral` textDisabled,
// `caution` cautionLine, `danger` danger), a 15/600 title (h1), one line of body in
// secondary text (max 420 wide), at most two buttons, and an optional 12 px caption
// under them. `busy` is the quiet in-between form (Connecting): a spinner and one line
// of secondary text, no icon, no title weight. The views own the logic and the words.

import type { ReactNode } from "react";

import { useTheme } from "../theme/ThemeContext";
import type { ThemeTokens } from "../theme/tokens";
import { Icon, type IconName } from "./Icon";
import { Spinner } from "./Spinner";

export type PageStateTone = "neutral" | "caution" | "danger";

export interface PageStateProps {
  /** The message: the title, or the one line beside the spinner when `busy`. */
  title: string;
  /** Default "neutral". */
  tone?: PageStateTone;
  /** The 36 px icon. Ignored when `busy`. */
  icon?: IconName;
  /** One line of body. */
  body?: ReactNode;
  /** At most two Buttons (the caller composes them). */
  actions?: ReactNode;
  /** A 12 px secondary caption under the buttons. */
  caption?: ReactNode;
  /** The in-between form: spinner + the title as one secondary line. */
  busy?: boolean;
}

function toneColor(t: ThemeTokens, tone: PageStateTone): string {
  switch (tone) {
    case "neutral":
      return t.textDisabled;
    case "caution":
      return t.cautionLine;
    case "danger":
      return t.danger;
  }
}

export function PageState({
  title,
  tone = "neutral",
  icon,
  body,
  actions,
  caption,
  busy = false,
}: PageStateProps) {
  const { t } = useTheme();
  return (
    <div
      style={{
        flex: 1,
        minHeight: 0,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        padding: t.sp24,
        fontFamily: t.fontSans,
        color: t.text,
      }}
    >
      {busy ? (
        <div
          role="status"
          style={{
            display: "flex",
            alignItems: "center",
            gap: t.sp8 + 2,
            fontSize: t.fontBody,
            color: t.textSecondary,
          }}
        >
          <Spinner size={16} stroke={t.textSecondary} strokeWidth={2} />
          {title}
        </div>
      ) : (
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            gap: t.sp8,
            textAlign: "center",
          }}
        >
          {icon && (
            <Icon
              name={icon}
              size={36}
              stroke={toneColor(t, tone)}
              strokeWidth={1.5}
            />
          )}
          <h1
            style={{
              margin: `${String(t.sp4)}px 0 0`,
              fontSize: t.fontLarge,
              lineHeight: `${String(t.lineLarge)}px`,
              fontWeight: t.weightSemibold,
            }}
          >
            {title}
          </h1>
          {body != null && (
            <p
              style={{
                margin: 0,
                maxWidth: 420,
                fontSize: t.fontBody,
                lineHeight: `${String(t.lineBody + 1)}px`,
                color: t.textSecondary,
              }}
            >
              {body}
            </p>
          )}
          {actions != null && (
            <div style={{ display: "flex", gap: t.sp8, marginTop: t.sp8 }}>
              {actions}
            </div>
          )}
          {caption != null && (
            <div
              style={{
                marginTop: t.sp6 - t.sp8,
                fontSize: t.fontCaption,
                lineHeight: `${String(t.lineCaption)}px`,
                color: t.textSecondary,
              }}
            >
              {caption}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
