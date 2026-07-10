// src/ui/Meter.tsx — a STATIC track+fill bar (no transition, no animation).
//
// The shared anatomy of the CPU meters (level/RowCpu · copy/CpuMeter): a rounded
// track with an absolutely-positioned fill sized to `pct`, plus an optional budget
// marker line. Fill/track colors and any over-budget logic stay at the call site
// (passed in). Deliberately NOT routed through ui/ProgressBar — that has a 0.4s
// transition; these meters must paint instantly.
import type { CSSProperties } from "react";
import { useTheme } from "../theme/ThemeContext";

export interface MeterProps {
  /** fill width as a percent; clamped to 0–100. */
  pct: number;
  width: number;
  height: number;
  /** track background; default t.track. */
  trackColor?: string;
  /** fill color; default t.accent. */
  fillColor?: string;
  /** optional budget-marker position (percent). */
  marker?: number;
  /** budget-marker color; default t.faint. */
  markerColor?: string;
}

export function Meter({
  pct,
  width,
  height,
  trackColor,
  fillColor,
  marker,
  markerColor,
}: MeterProps) {
  const { t } = useTheme();
  const w = Math.max(0, Math.min(100, pct));
  const track: CSSProperties = {
    position: "relative",
    width,
    height,
    borderRadius: t.rPill,
    background: trackColor ?? t.track,
    overflow: "hidden",
  };
  return (
    <span style={track}>
      <span
        style={{
          position: "absolute",
          left: 0,
          top: 0,
          bottom: 0,
          width: `${String(w)}%`,
          borderRadius: t.rPill,
          background: fillColor ?? t.accent,
        }}
      />
      {marker != null && (
        <span
          style={{
            position: "absolute",
            left: `${String(marker)}%`,
            top: -1,
            bottom: -1,
            width: 1.5,
            background: markerColor ?? t.faint,
          }}
        />
      )}
    </span>
  );
}
