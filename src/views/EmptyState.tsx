// src/views/EmptyState.tsx — in-body "no device" empty state.
//
// Used by the device-sourced pages (Presets, Songs): when the unit is absent
// there is nothing to list, so the body becomes this deliberate empty state
// while the nav stays live above it. Ported from the prototype `EmptyState`
// (handoff reference_prototype/screens.jsx).

import type { ReactNode } from "react";
import { useTheme } from "../theme/ThemeContext";
import { Icon } from "../ui/Icon";
import { PageNotice } from "./PageNotice";

export interface EmptyStateProps {
  title: string;
  body: ReactNode;
  hint?: ReactNode;
  /** "Scan for device" — re-trigger a connection attempt. */
  onScan?: () => void;
  /** "Connection help" — optional secondary action. */
  onHelp?: () => void;
  cta?: string;
}

export function EmptyState({
  title,
  body,
  hint,
  onScan,
  onHelp,
  cta = "Scan for device",
}: EmptyStateProps) {
  const { t } = useTheme();
  return (
    <PageNotice
      tone="neutral"
      medallion={
        <>
          <Icon name="cable" size={32} stroke={t.faint} strokeWidth={1.4} />
          <span
            style={{
              position: "absolute",
              right: -3,
              bottom: -3,
              width: 24,
              height: 24,
              borderRadius: t.rPill,
              background: t.bg,
              border: `0.5px solid ${t.hairlineStrong}`,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
            }}
          >
            <Icon name="x" size={12} stroke={t.mutedInk} strokeWidth={2} />
          </span>
        </>
      }
      kicker="No device"
      title={title}
      body={body}
      hint={hint}
      primary={{ label: cta, icon: "refresh", onClick: onScan }}
      secondary={{ label: "Connection help", onClick: onHelp }}
    />
  );
}

/** Inline mono USB-C token, for use inside EmptyState bodies. */
export function UsbC() {
  const { t } = useTheme();
  return (
    <span style={{ fontFamily: t.mono, fontSize: t.fsControl, color: t.ink2 }}>
      USB-C
    </span>
  );
}
