// src/ui/RunFooterStatus.tsx — the primary (right) slot of a run page's footer
// (#314), shared by the Doctor check (`doctor/DoctorRun`) and the Level wizard's run
// (`level/RunPage`).
//
// Three states, checked in this order:
//   · stopped — a manual stop landed: ONE primary CTA (`ctaLabel` — "See results" /
//     "Continue") that moves on; there is no auto-advance after a stop.
//   · done — a natural finish: a static "done" marker, NOT a button, because
//     `useAutoAdvance` is already moving on a beat later (a live CTA would only flash).
//   · otherwise — the caller's own `running` content (its Stop button, its
//     "finishing current item…" wind-down, or a caption).
// Each page keeps its own state machine; this owns only the shared marker and CTA.

import type { ReactNode } from "react";

import { useTheme } from "../theme/ThemeContext";
import { Icon } from "./Icon";
import { Button } from "./primitives";

export interface RunFooterStatusProps {
  /** A manual stop has landed — show the CTA. Wins over `done`. */
  stopped: boolean;
  /** The run finished on its own — show the static done marker. */
  done: boolean;
  /** The stopped CTA's label ("See results" / "Continue"). */
  ctaLabel: string;
  onCta: () => void;
  /** Shown while the run is neither stopped nor done. */
  running: ReactNode;
}

export function RunFooterStatus({
  stopped,
  done,
  ctaLabel,
  onCta,
  running,
}: RunFooterStatusProps) {
  const { t } = useTheme();
  if (stopped) {
    return (
      <Button
        variant="primary"
        size="sm"
        onClick={onCta}
        style={{ height: 32, padding: `0 ${String(t.space8)}px` }}
      >
        {ctaLabel}
      </Button>
    );
  }
  if (done) {
    return (
      <span
        style={{
          display: "inline-flex",
          alignItems: "center",
          gap: t.space4,
          fontFamily: t.mono,
          fontSize: 10.5,
          letterSpacing: "0.04em",
          color: t.mutedInk,
        }}
      >
        <Icon name="check" size={13} stroke={t.good} strokeWidth={2} />
        done
      </span>
    );
  }
  return <>{running}</>;
}
