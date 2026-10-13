// src/views/signalChain/SplitGroup.tsx — an in-line split: two parallel branches
// between a SPLIT and a MIX diamond, joined by measured rounded brackets.

import type { ReactNode } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { DiamondNode } from "./DiamondNode";
import { LaneBracketCap } from "./LaneBracketCap";
import { STROKE_W, type StripBlock } from "./stripTypes";
import { useLaneBracket } from "./useLaneBracket";

export interface SplitGroupProps {
  a: StripBlock[];
  b: StripBlock[];
  ink: string;
  laneGap: number;
  skeleton?: boolean;
  pfx: string;
  renderRow: (arr: StripBlock[], pfx: string) => ReactNode[];
}

// The two parallel branches between a SPLIT and a MIX diamond, joined by the
// rounded brackets. Branch heights vary (a half-stack tile is much taller than a
// pedal branch), so the brackets are MEASURED from the rendered branch art centres
// at runtime rather than from a fixed tile height — for equal-height branches this
// resolves to the same geometry the old fixed brackets used.
export function SplitGroup({
  a,
  b,
  ink,
  laneGap,
  skeleton,
  pfx,
  renderRow,
}: SplitGroupProps) {
  const { t } = useTheme();
  const { colRef, aRef, bRef, brk } = useLaneBracket();

  // The straight-through line for an EMPTY branch, painted here (not by
  // `EmptyLane`) so it lands on the SAME measured y as the bracket's top/bottom
  // border and reads as one continuous stroke. `atTop` picks the branch: the top
  // matches the border-box bracket's borderTop at [brk.top, brk.top+1.5]; the
  // bottom matches its borderBottom at [brk.top+brk.height-1.5, brk.top+brk.height].
  const emptyLine = (atTop: boolean) =>
    brk ? (
      <div
        style={{
          position: "absolute",
          left: 0,
          right: 0,
          top: atTop ? brk.top : brk.top + brk.height - STROKE_W,
          height: STROKE_W,
          background: ink,
        }}
      />
    ) : null;

  return (
    <>
      <DiamondNode kind="split" ink={ink} skeleton={skeleton} />
      <div
        ref={colRef}
        style={{
          display: "flex",
          flexDirection: "column",
          gap: laneGap,
          position: "relative",
          marginLeft: t.space6,
          marginRight: t.space6,
        }}
      >
        <LaneBracketCap brk={brk} side="left" ink={ink} />
        <LaneBracketCap brk={brk} side="right" ink={ink} />
        {a.length === 0 && emptyLine(true)}
        {b.length === 0 && emptyLine(false)}
        <div
          ref={aRef}
          style={{ display: "flex", alignItems: "stretch", gap: t.space5 }}
        >
          {renderRow(a, `${pfx}a`)}
        </div>
        <div
          ref={bRef}
          style={{ display: "flex", alignItems: "stretch", gap: t.space5 }}
        >
          {renderRow(b, `${pfx}b`)}
        </div>
      </div>
      <DiamondNode kind="mix" ink={ink} skeleton={skeleton} />
    </>
  );
}
