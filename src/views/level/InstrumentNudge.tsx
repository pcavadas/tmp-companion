// src/views/level/InstrumentNudge.tsx — the quiet good → better → best caption under
// the Set up page's bulk Guitar picker, with its inline "Calibrate" cue (a private
// helper: it only ever renders inside this caption).

import { useState, type ReactNode } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { Button } from "../../ui/primitives";

interface CalibrateCueProps {
  children: ReactNode;
  onCalibrate?: () => void;
}

/** The "calibrate" word — an inviting next-step cue. Dotted terracotta underline
 *  that solidifies on hover; clicking jumps to Settings → Instruments. Click-only
 *  app, so no keyboard handler — the click IS the affordance. */
function CalibrateCue({ children, onCalibrate }: CalibrateCueProps) {
  const { t } = useTheme();
  const [hover, setHover] = useState(false);
  return (
    <Button
      variant="bare"
      title="Calibrate instruments in Settings"
      onClick={onCalibrate}
      onMouseEnter={() => {
        setHover(true);
      }}
      onMouseLeave={() => {
        setHover(false);
      }}
      style={{
        color: t.accentDeep,
        fontWeight: 500,
        textDecoration: "underline",
        textDecorationStyle: "dotted",
        textDecorationColor: hover ? t.accentDeep : t.warnBorder,
        textUnderlineOffset: "2.5px",
      }}
    >
      {children}
    </Button>
  );
}

export interface InstrumentNudgeProps {
  state: "none" | "uncal" | "cal";
  onCalibrate?: () => void;
}

/** Quiet good → better → best caption beneath the bulk instrument picker. `cal`
 *  removes the element entirely so the list below reclaims the space. */
export function InstrumentNudge({ state, onCalibrate }: InstrumentNudgeProps) {
  const { t } = useTheme();
  if (state === "cal") return null;
  return (
    <div
      aria-live="polite"
      style={{
        marginTop: t.space3,
        fontFamily: t.sans,
        fontSize: 11.5,
        lineHeight: 1.45,
        color: t.mutedInk,
        textWrap: "pretty",
      }}
    >
      {state === "none" ? (
        <span>
          Set an instrument for better results.{" "}
          <CalibrateCue onCalibrate={onCalibrate}>Calibrate</CalibrateCue> it to
          get more accurate levels.
        </span>
      ) : (
        <span>
          <CalibrateCue onCalibrate={onCalibrate}>Calibrate</CalibrateCue> this
          instrument for more accurate levels.
        </span>
      )}
    </div>
  );
}
