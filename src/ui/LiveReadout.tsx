// src/ui/LiveReadout.tsx — a compact live metric readout: a large tabular-nums value with
// a small unit suffix and an uppercase caption beneath. Presentational + token-only; the
// caller supplies the formatter, unit, and caption so the primitive stays domain-free.

import { useStyles, useTheme } from "../theme/ThemeContext";

export interface LiveReadoutProps {
  /** The current numeric value. */
  value: number;
  /** Formats `value` for display (default: `String`). */
  format?: (n: number) => string;
  /** Small unit suffix beside the value (e.g. "LUFS"). */
  unit?: string;
  /** Uppercase caption beneath the value (e.g. "measuring…"). */
  caption?: string;
}

export function LiveReadout({
  value,
  format = String,
  unit,
  caption,
}: LiveReadoutProps) {
  const { t } = useTheme();
  const s = useStyles();
  return (
    <div style={{ flexShrink: 0, textAlign: "right", minWidth: 104 }}>
      <div style={{ lineHeight: 1 }}>
        <span
          style={{
            fontFamily: t.mono,
            fontSize: 19,
            fontWeight: 500,
            color: t.accentDeep,
            fontVariantNumeric: "tabular-nums",
          }}
        >
          {format(value)}
        </span>
        {unit != null && (
          <span
            style={{
              fontFamily: t.mono,
              fontSize: 9.5,
              color: t.faint,
              marginLeft: 4,
            }}
          >
            {unit}
          </span>
        )}
      </div>
      {caption != null && (
        <div style={{ ...s.kicker(t.sevWarn), marginTop: 5 }}>{caption}</div>
      )}
    </div>
  );
}

export default LiveReadout;
