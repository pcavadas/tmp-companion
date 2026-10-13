// src/views/signalChain/ForkTail.tsx — split-OUTPUT lanes: one SPLIT diamond feeding
// two lanes that each end in their own OUT jack.

import type { ReactNode } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { DiamondNode } from "./DiamondNode";
import { EndpointNode } from "./EndpointNode";
import { LaneBracketCap } from "./LaneBracketCap";
import type { Size, StripBlock, StripGraph } from "./stripTypes";
import { useLaneBracket } from "./useLaneBracket";
import { Wire } from "./Wire";

export interface ForkTailProps {
  outputs: NonNullable<StripGraph["outputs"]>;
  ink: string;
  size: Size;
  skeleton?: boolean;
  laneGap: number;
  renderRow: (arr: StripBlock[], pfx: string) => ReactNode[];
}

// Split-OUTPUT lanes (gtrSplit/micSplit): one SPLIT diamond feeds two
// independent lanes that each terminate in their own physical OUT — unlike
// SplitGroup's in-line split, there's no MIX diamond on the far side, so only
// one bracket (on the split side) is needed. Lanes may hold a different
// number of blocks (HW-confirmed on a real "Instrument Split" preset): the
// column wrapper stretches both rows to a common width (the flex column
// default) so the two OUT jacks land on the same column regardless of lane
// length, but the STRETCH is absorbed by a trailing growing `Wire` right
// before SinkNode — not by the leading gap — so the tiles stay anchored
// immediately after the split instead of being shoved rightward.
export function ForkTail({
  outputs,
  ink,
  size,
  skeleton,
  laneGap,
  renderRow,
}: ForkTailProps) {
  const { t } = useTheme();
  const { colRef, aRef, bRef, brk } = useLaneBracket();
  const lanes = [outputs.a, outputs.b];
  const laneRefs = [aRef, bRef];
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
        }}
      >
        <LaneBracketCap brk={brk} side="left" ink={ink} />
        {lanes.map((lane, li) => (
          <div
            key={li}
            ref={laneRefs[li]}
            style={{ display: "flex", alignItems: "stretch", gap: t.space5 }}
          >
            {lane.blocks.length > 0 && renderRow(lane.blocks, `o${String(li)}`)}
            <Wire ink={ink} grow w={lane.blocks.length > 0 ? 10 : 18} />
            <EndpointNode
              type={lane.type}
              ink={ink}
              skeleton={skeleton}
              size={size}
            />
          </div>
        ))}
      </div>
    </>
  );
}
