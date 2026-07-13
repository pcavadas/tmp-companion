// src/views/overlays/WizardShell.tsx — the leveling wizard's stable chrome.
//
// A leveling run is one linear task, so it lives in ONE mounted frame whose BODY
// swaps per stage — the frame never resizes or re-centers. The header is a 3-step
// rail (Set up · Level · Summary), the footer keeps the same two slots (secondary
// left · primary right) on every step. Backdrop click closes on every stage EXCEPT
// the run (never abort an in-progress device operation).
//
// Reuses the kit: APP tokens (useTheme), Icon, the tmp-spin keyframes.

import { Fragment, type ReactNode } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Dialog, DialogFooter, DIALOG_PAD_X } from "../../ui/Dialog";
import { Icon } from "../../ui/Icon";
import { WIZ_STEPS, type Stage, type RailStep } from "./wizardContext";

// Re-export the wizard's shared TYPES so existing importers keep working; the
// VALUE exports (DialogCardCtx, stageToStep, WIZ_STEPS) live in ./wizardContext.
export type { Stage, RailStep };

// ---- step rail (header) ----------------------------------------------------
// Exported so the full-page LevelSetupPage reuses the identical rail. `steps`
// defaults to the leveling rail.
export function StepRail({
  current,
  steps = WIZ_STEPS,
}: {
  current: number;
  steps?: readonly RailStep[];
}) {
  const { t } = useTheme();
  return (
    <div style={{ display: "flex", alignItems: "center" }}>
      {steps.map((s, i) => {
        const done = i < current;
        const active = i === current;
        const filled = done || active;
        return (
          <Fragment key={s.key}>
            <div
              style={{ display: "flex", alignItems: "center", gap: t.space4 }}
            >
              <span
                style={{
                  width: 22,
                  height: 22,
                  borderRadius: 999,
                  flexShrink: 0,
                  display: "inline-flex",
                  alignItems: "center",
                  justifyContent: "center",
                  background: filled ? t.accent : "transparent",
                  border: `1px solid ${filled ? t.accent : t.hairlineStrong}`,
                  fontFamily: t.mono,
                  fontSize: 11,
                  fontWeight: 500,
                  color: filled ? t.onInk : t.faint,
                }}
              >
                {done ? (
                  <Icon
                    name="check"
                    size={12}
                    stroke={t.onInk}
                    strokeWidth={2.4}
                  />
                ) : (
                  i + 1
                )}
              </span>
              <span
                style={{
                  fontFamily: t.mono,
                  fontSize: 10.5,
                  letterSpacing: "0.1em",
                  textTransform: "uppercase",
                  color: active ? t.ink : done ? t.mutedInk : t.faint,
                  whiteSpace: "nowrap",
                }}
              >
                {s.label}
              </span>
            </div>
            {i < steps.length - 1 && (
              <span
                style={{
                  flex: 1,
                  height: 1,
                  margin: `0 ${String(t.space6)}px`,
                  background: i < current ? t.accent : t.hairlineStrong,
                  minWidth: 16,
                }}
              />
            )}
          </Fragment>
        );
      })}
    </div>
  );
}

// ---- rail header (the bgAlt bar that frames the StepRail) -------------------
// Shared by the modal WizardShell and the full-page LevelSetupPage so the header
// styling lives in ONE place; only the outer container (centered modal vs opaque
// inset:0 page) differs between the two.
export function WizardHeader({
  current,
  steps,
}: {
  current: number;
  steps?: readonly RailStep[];
}) {
  const { t } = useTheme();
  return (
    <div
      style={{
        flexShrink: 0,
        padding: `${String(t.space8)}px ${String(DIALOG_PAD_X)}px`,
        borderBottom: `0.5px solid ${t.hairline}`,
        background: t.bgAlt,
      }}
    >
      <StepRail current={current} steps={steps} />
    </div>
  );
}

// ---- the fixed frame -------------------------------------------------------
export interface WizardShellProps {
  current: number;
  /** Dismiss handler for a backdrop click — pass undefined to make the scrim inert
   *  (the run stage, so a stray click never aborts a device operation). */
  onBackdrop?: () => void;
  children: ReactNode;
  /** Fixed frame height (px). Default 512. */
  height?: number;
  /** Rail nodes (defaults to the leveling rail). Bulk Block Edit passes its own
   *  4-node rail (Block · Choose · Check · Done). */
  steps?: readonly RailStep[];
}

export function WizardShell({
  current,
  onBackdrop,
  children,
  height = 512,
  steps = WIZ_STEPS,
}: WizardShellProps) {
  return (
    <Dialog size="md" height={height} onClose={onBackdrop}>
      <WizardHeader current={current} steps={steps} />
      {children}
    </Dialog>
  );
}

// ---- footer (two fixed slots) ----------------------------------------------
export interface WizardFooterProps {
  left: ReactNode;
  right: ReactNode;
}

export function WizardFooter({ left, right }: WizardFooterProps) {
  // The wizard's two-slot footer is the DS DialogFooter with a left group (pushed
  // left by the `start` slot) and the primary actions on the right.
  const { t } = useTheme();
  return (
    <DialogFooter
      start={
        <div style={{ display: "flex", alignItems: "center", gap: t.space4 }}>
          {left}
        </div>
      }
    >
      <div style={{ display: "flex", alignItems: "center", gap: t.space4 }}>
        {right}
      </div>
    </DialogFooter>
  );
}

// ---- serif body title (each stage's header) --------------------------------
export function WizTitle({
  children,
  size = 22,
  style,
}: {
  children: ReactNode;
  size?: number;
  style?: React.CSSProperties;
}) {
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
