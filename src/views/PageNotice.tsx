// src/views/PageNotice.tsx — the shared full-page centered-notice shell.
//
// One body-filling "designed empty/notice state": guide-stripe backdrop, a 72px
// medallion, kicker → title → body, an action row, and an optional hint. Two
// callers compose it — EmptyState (neutral "no device") and FirmwareGate (warn
// "untested firmware") — so the layout lives in exactly one place. The medallion
// inner content is a slot (EmptyState's is a composite icon+badge, FirmwareGate's
// a single icon); `tone` drives the medallion border/fill + kicker color.

import type { ReactNode } from "react";
import { useTheme } from "../theme/ThemeContext";
import { Button } from "../ui/primitives";
import type { IconName } from "../ui/iconNames";

export interface PageNoticeAction {
  label: string;
  icon?: IconName;
  onClick?: () => void;
}

export interface PageNoticeProps {
  /** Inner content of the 72px medallion (an Icon, or icon + composite badge). */
  medallion: ReactNode;
  /** Medallion border + fill + kicker tone. */
  tone?: "neutral" | "warn";
  kicker: string;
  title: ReactNode;
  body: ReactNode;
  primary?: PageNoticeAction;
  secondary?: PageNoticeAction;
  hint?: ReactNode;
  /** Body column max width (px). */
  bodyMaxWidth?: number;
}

export function PageNotice({
  medallion,
  tone = "neutral",
  kicker,
  title,
  body,
  primary,
  secondary,
  hint,
  bodyMaxWidth = 410,
}: PageNoticeProps) {
  const { t } = useTheme();
  const warn = tone === "warn";

  return (
    <div
      style={{
        flex: 1,
        minHeight: 0,
        position: "relative",
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        background: t.bg,
        padding: "0 48px",
        textAlign: "center",
        overflow: "hidden",
      }}
    >
      {/* faint guide stripes, so it reads as a designed state */}
      <div
        style={{
          position: "absolute",
          inset: 0,
          backgroundImage: `repeating-linear-gradient(135deg, ${t.hairline} 0 1px, transparent 1px 13px)`,
          opacity: 0.5,
          pointerEvents: "none",
        }}
      />

      <div
        style={{
          position: "relative",
          width: 72,
          height: 72,
          borderRadius: t.rPill,
          border: `0.5px solid ${warn ? t.warn : t.hairlineStrong}`,
          background: warn ? t.warnSoft : t.bgAlt,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          marginBottom: 24,
        }}
      >
        {medallion}
      </div>

      <div
        style={{
          position: "relative",
          fontFamily: t.mono,
          fontSize: t.fsData2,
          letterSpacing: "0.18em",
          color: warn ? t.warn : t.faint,
          textTransform: "uppercase",
        }}
      >
        {kicker}
      </div>
      <div
        style={{
          position: "relative",
          fontFamily: t.serif,
          fontSize: t.fsTitle,
          color: t.ink,
          marginTop: 11,
          letterSpacing: t.lsTight,
        }}
      >
        {title}
      </div>
      <div
        style={{
          position: "relative",
          fontFamily: t.sans,
          fontSize: t.fsBody2,
          lineHeight: 1.6,
          color: t.mutedInk,
          marginTop: 12,
          maxWidth: bodyMaxWidth,
        }}
      >
        {body}
      </div>

      {(primary ?? secondary) && (
        <div
          style={{
            position: "relative",
            display: "flex",
            gap: 10,
            marginTop: 24,
            alignItems: "center",
          }}
        >
          {primary && (
            <Button
              variant="primary"
              icon={primary.icon}
              onClick={primary.onClick}
            >
              {primary.label}
            </Button>
          )}
          {secondary && (
            <Button variant="ghost" onClick={secondary.onClick}>
              {secondary.label}
            </Button>
          )}
        </div>
      )}

      {hint && (
        <div
          style={{
            position: "relative",
            marginTop: 28,
            display: "flex",
            alignItems: "center",
            gap: 9,
            fontFamily: t.mono,
            fontSize: t.fsMeta,
            color: t.faint,
          }}
        >
          <span
            style={{
              width: 6,
              height: 6,
              borderRadius: t.rPill,
              background: t.faint,
            }}
          />
          {hint}
        </div>
      )}
    </div>
  );
}
