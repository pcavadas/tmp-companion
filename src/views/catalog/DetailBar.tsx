// src/views/catalog/DetailBar.tsx — the chosen model's details (look B), shown only
// while a model is chosen: its art (72 × 72), name, what it models, a row of facts
// (Type, CPU, Sound, Engine for reverbs, Since) and a close button that clears the
// choice. `bar` pins it under the wall (a white band with a rule on top); `column`
// stands it beside the wall in a wide window (300 px, a rule on the left, stacked).

import type { ReactNode } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { sectionLabel } from "../../theme/tokens";
import { Button } from "../../ui/primitives";
import { CPU_BUDGET, cpuStr } from "../../models/cpu";
import { displayName, type ModelRecord } from "../../models/catalog";
import { modelsLine, typeFact } from "./catalogLabels";
import { ModelTile } from "./ModelTile";

/** The detail's art box. */
const DETAIL_ART = 72;
/** The side column's width in a wide window. */
export const DETAIL_COLUMN_W = 300;

interface FactProps {
  label: string;
  children: ReactNode;
}

function Fact({ label, children }: FactProps) {
  const { t } = useTheme();
  return (
    <span style={{ display: "flex", flexDirection: "column", gap: t.sp2 }}>
      <span style={sectionLabel(t)}>{label}</span>
      <span
        style={{
          fontSize: t.fontBody,
          lineHeight: `${String(t.lineBody)}px`,
          fontVariantNumeric: "tabular-nums",
          whiteSpace: "nowrap",
        }}
      >
        {children}
      </span>
    </span>
  );
}

export interface DetailBarProps {
  r: ModelRecord;
  onClose: () => void;
  /** `bar` under the wall (default), `column` beside it. */
  layout?: "bar" | "column";
}

export function DetailBar({ r, onClose, layout = "bar" }: DetailBarProps) {
  const { t } = useTheme();
  const column = layout === "column";
  const facts: [string, string][] = [["Type", typeFact(r)]];
  if (r.cpu != null)
    facts.push(["CPU", `${cpuStr(r.cpu)} of ${String(CPU_BUDGET)}%`]);
  facts.push(["Sound", r.ch === "stereo" ? "Stereo" : "Mono"]);
  if (r.cat === "Effects" && r.sub === "Reverb")
    facts.push(["Engine", r.conv ? "Convolution" : "Algorithmic"]);
  facts.push(["Since", `Firmware ${r.since}`]);

  const close = (
    <Button
      variant="plain"
      size="sm"
      icon="x"
      aria-label="Close details"
      onClick={onClose}
    />
  );

  return (
    <section
      aria-label={`Details: ${displayName(r)}`}
      style={{
        flexShrink: 0,
        boxSizing: "border-box",
        display: "flex",
        flexDirection: column ? "column" : "row",
        alignItems: "flex-start",
        gap: t.sp16,
        width: column ? DETAIL_COLUMN_W : undefined,
        padding: column
          ? `${String(t.sp16)}px ${String(t.sp24)}px`
          : `${String(t.sp12 + 2)}px ${String(t.sp24)}px`,
        background: t.surface,
        borderTop: column ? undefined : `1px solid ${t.border}`,
        borderLeft: column ? `1px solid ${t.border}` : undefined,
        overflowY: column ? "auto" : undefined,
        fontFamily: t.fontSans,
        color: t.text,
      }}
    >
      {column ? (
        <div
          style={{
            alignSelf: "stretch",
            display: "flex",
            alignItems: "flex-start",
            justifyContent: "space-between",
          }}
        >
          <ModelTile r={r} size={DETAIL_ART} />
          {close}
        </div>
      ) : (
        <ModelTile r={r} size={DETAIL_ART} />
      )}
      <div
        style={{
          flex: column ? undefined : 1,
          minWidth: 0,
          display: "flex",
          flexDirection: "column",
          gap: t.sp4,
        }}
      >
        <h2
          style={{
            margin: 0,
            fontSize: t.fontLarge,
            lineHeight: `${String(t.lineLarge)}px`,
            fontWeight: t.weightSemibold,
          }}
        >
          {displayName(r)}
        </h2>
        <span
          style={{
            fontSize: t.fontBody,
            lineHeight: `${String(t.lineBody)}px`,
            color: t.textSecondary,
          }}
        >
          {modelsLine(r)}
        </span>
        <div
          style={{
            display: "flex",
            flexDirection: column ? "column" : "row",
            flexWrap: "wrap",
            columnGap: t.sp24,
            rowGap: column ? t.sp12 : t.sp8,
            marginTop: t.sp6,
          }}
        >
          {facts.map(([k, v]) => (
            <Fact key={k} label={k}>
              {v}
            </Fact>
          ))}
        </div>
      </div>
      {!column && close}
    </section>
  );
}
