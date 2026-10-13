// src/views/signalChain/JoinHead.tsx — dual-INPUT lanes: two source lanes converging
// into one JOIN diamond (the mirror of ForkTail).

import type { ReactNode } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { DiamondNode } from "./DiamondNode";
import { EndpointNode } from "./EndpointNode";
import { LaneBracketCap } from "./LaneBracketCap";
import type { Size, StripBlock, StripGraph } from "./stripTypes";
import { useLaneBracket } from "./useLaneBracket";
import { Wire } from "./Wire";

export interface JoinHeadProps {
  inputs: NonNullable<StripGraph["inputs"]>;
  ink: string;
  size: Size;
  skeleton?: boolean;
  laneGap: number;
  renderRow: (arr: StripBlock[], pfx: string) => ReactNode[];
}

// Dual-INPUT lanes (gtrMicSeries/gtrMicMix/etc.): mirror of ForkTail — two
// independent lanes converge into one JOIN diamond, so the single bracket
// sits on the join side. Same stretch-absorbed-by-a-growing-Wire rationale as
// ForkTail applies here, mirrored: the growing Wire sits right after
// SourceNode so the two source jacks stay aligned regardless of lane length.
export function JoinHead({
  inputs,
  ink,
  size,
  skeleton,
  laneGap,
  renderRow,
}: JoinHeadProps) {
  const { t } = useTheme();
  const { colRef, aRef, bRef, brk } = useLaneBracket();
  const lanes = [inputs.a, inputs.b];
  const laneRefs = [aRef, bRef];
  return (
    <>
      <div
        ref={colRef}
        style={{
          display: "flex",
          flexDirection: "column",
          gap: laneGap,
          position: "relative",
          marginRight: t.space6,
        }}
      >
        <LaneBracketCap brk={brk} side="right" ink={ink} />
        {lanes.map((lane, li) => (
          <div
            key={li}
            ref={laneRefs[li]}
            style={{ display: "flex", alignItems: "stretch", gap: t.space5 }}
          >
            <EndpointNode
              type={lane.type}
              ink={ink}
              skeleton={skeleton}
              size={size}
            />
            <Wire ink={ink} grow w={lane.blocks.length > 0 ? 10 : 18} />
            {lane.blocks.length > 0 && renderRow(lane.blocks, `i${String(li)}`)}
          </div>
        ))}
      </div>
      <DiamondNode kind="join" ink={ink} skeleton={skeleton} />
    </>
  );
}
