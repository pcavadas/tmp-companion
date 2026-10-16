// src/ui/Meter.tsx — the CPU meter (look B): a STATIC 40×4 bar (no transition, no
// animation) with the figure to its right ("29.0%", tabular numbers).
//
// The bar is BUDGET-relative: its fill is the preset's load as a share of the device's
// per-preset budget (`CPU_BUDGET`), so a preset at 29.0% of the audio core draws at 38%.
// Over the budget — by the SAME rule Copy's validation and the device apply,
// `overCpuBudget` (the total rounded half away from zero must not exceed the budget, so
// exactly 76.5 is over) — the fill and the figure turn `danger` and the word "Over"
// follows the figure, so the state never rests on colour alone.
//
// Deliberately NOT routed through ui/ProgressBar — that has a 0.4s transition; this meter
// must paint instantly.

import { useTheme } from "../theme/ThemeContext";
import { CPU_BUDGET, cpuStr, overCpuBudget } from "../models/cpu";

/** Bar size (px). */
const BAR_W = 40;
const BAR_H = 4;

export interface MeterProps {
  /** The preset's DSP load, % of the audio core (e.g. 29.0). */
  value: number;
  /** Optional tooltip on the whole meter. */
  title?: string;
}

/** The bar's fill as a % of its width: `value` against the budget, clamped 0–100. */
function meterFill(value: number): number {
  return Math.max(0, Math.min(100, (value / CPU_BUDGET) * 100));
}

export function Meter({ value, title }: MeterProps) {
  const { t } = useTheme();
  const over = overCpuBudget(value);
  const tone = over ? t.danger : t.meter;
  return (
    <span
      title={title}
      style={{
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "flex-end",
        gap: t.sp8,
        flexShrink: 0,
        fontFamily: t.fontSans,
      }}
    >
      <span
        aria-hidden
        style={{
          position: "relative",
          display: "block",
          width: BAR_W,
          height: BAR_H,
          borderRadius: BAR_H / 2,
          background: t.track,
          overflow: "hidden",
        }}
      >
        <span
          style={{
            position: "absolute",
            left: 0,
            top: 0,
            bottom: 0,
            width: `${String(meterFill(value))}%`,
            background: tone,
          }}
        />
      </span>
      <span
        style={{
          minWidth: 40,
          textAlign: "right",
          fontSize: t.fontCaption,
          color: over ? t.danger : t.textSecondary,
          fontVariantNumeric: "tabular-nums",
          whiteSpace: "nowrap",
        }}
      >
        {cpuStr(value)}
      </span>
      {over && (
        <span
          style={{
            fontSize: t.fontCaption,
            fontWeight: t.weightSemibold,
            color: t.danger,
          }}
        >
          Over
        </span>
      )}
    </span>
  );
}
