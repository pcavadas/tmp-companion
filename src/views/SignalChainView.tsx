// src/views/SignalChainView.tsx — signal-chain strip geometry for all TMP path
// template families. Blocks render through the shared BlockArt SVG engine so
// the active strip and Models tab stay identical by construction. This file lays
// the strip out; its pieces (tiles, jacks, diamonds, wires, the measured split/join
// brackets) and the StripGraph model live in ./signalChain/.

import { memo, type CSSProperties, type Key, type ReactNode } from "react";

import { useTheme } from "../theme/ThemeContext";
import { BlockTile } from "./signalChain/BlockTile";
import { EmptyLane } from "./signalChain/EmptyLane";
import { EndpointNode } from "./signalChain/EndpointNode";
import { ForkTail } from "./signalChain/ForkTail";
import { JoinHead } from "./signalChain/JoinHead";
import { SplitGroup } from "./signalChain/SplitGroup";
import { Wire } from "./signalChain/Wire";
import type { Size, StripBlock, StripGraph } from "./signalChain/stripTypes";

// The strip's data model lives in ./signalChain/stripTypes; re-exported here so every
// caller keeps importing it beside the component.
export type {
  SinkType,
  SourceType,
  StripBlock,
  StripGraph,
  StripIndependentLane,
  StripInputLane,
  StripOutputLane,
  StripStage,
  TileChange,
} from "./signalChain/stripTypes";

export interface SignalChainViewProps {
  graph: StripGraph;
  size?: Size;
  skeleton?: boolean;
}

function SignalChainViewImpl({
  graph,
  size = "md",
  skeleton,
}: SignalChainViewProps) {
  const { t } = useTheme();
  const ink = skeleton ? t.stripGhost : t.stripWire;
  const laneGap = 10;

  const tile = (b: StripBlock, key: Key) => (
    <BlockTile key={key} b={b} size={size} skeleton={skeleton} />
  );
  const row = (arr: StripBlock[], pfx: string): ReactNode[] =>
    arr.length > 0
      ? arr.flatMap((b, i) =>
          i === 0
            ? [tile(b, `${pfx}t${String(i)}`)]
            : [
                <Wire key={`${pfx}w${String(i)}`} ink={ink} />,
                tile(b, `${pfx}t${String(i)}`),
              ],
        )
      : [<EmptyLane key={`${pfx}empty`} size={size} />];

  const splitSection = (
    st: { a: StripBlock[]; b: StripBlock[] },
    key: string,
  ) => (
    <SplitGroup
      key={key}
      pfx={key}
      a={st.a}
      b={st.b}
      ink={ink}
      laneGap={laneGap}
      skeleton={skeleton}
      renderRow={row}
    />
  );

  // Inner strip — the handoff geometry verbatim (padding "2px 8px 4px"), with NO overflow
  // of its own so its overflow-y stays `visible` and the Copy edit badge (top:-7) paints
  // freely above series tiles.
  const stripStyle: CSSProperties = {
    display: "flex",
    // Each block is its own column (art + an 18px label zone). `center` keeps every
    // column at its natural height so each label hugs its own block; `stretch` grew
    // short columns to the tallest (a split group), dropping their labels onto a
    // shared bottom baseline far below the art (design drift #1). Lane rows inside a
    // split stay `stretch` — they want equal-height branches for the bracket math.
    alignItems: "center",
    // `safe center` centers the strip when it fits, but falls back to flex-start
    // when the chain overflows — so a long chain scrolls from its FIRST block
    // (plain `center` clips the left end out of the scroll range: the first tile,
    // e.g. SWELL, became unreachable and the next was cropped).
    justifyContent: "safe center",
    gap: t.space5,
    // Right clears the added-block badge's right:-5 overhang; bottom gives the slim
    // scrollbar room. Top stays small — the stripScroll wrapper adds the badge room.
    padding: `${String(t.space1)}px ${String(t.space5)}px ${String(t.space4)}px`,
  };

  // The horizontal scroll lives on this wrapper, not the strip: an overflowX:auto box
  // coerces overflow-y to `auto` (CSS spec), which would clip the badge poking above a
  // slack-less series tile. paddingTop gives the badge room; the matching negative
  // marginTop cancels the shift so neither the Copy nor Level strip moves a pixel.
  // The two MUST stay equal (both space4) — they cancel to net zero.
  const stripScroll: CSSProperties = {
    overflowX: "auto",
    width: "fit-content",
    maxWidth: "100%",
    margin: `-${String(t.space4)}px auto 0`,
    paddingTop: t.space4,
  };
  if (graph.lanes && graph.lanes.length > 0) {
    return (
      <div style={stripScroll} className="tmp-block-strip">
        <div
          style={{
            ...stripStyle,
            flexDirection: "column",
            alignItems: "center",
            gap: laneGap,
          }}
        >
          {graph.lanes.map((lane, li) => (
            <div
              key={li}
              style={{ display: "flex", alignItems: "stretch", gap: t.space5 }}
            >
              <EndpointNode
                type={lane.input}
                ink={ink}
                skeleton={skeleton}
                size={size}
              />
              <Wire ink={ink} />
              {lane.blocks.length > 0 ? (
                <>
                  {row(lane.blocks, `ln${String(li)}`)}
                  <Wire ink={ink} />
                </>
              ) : (
                <Wire w={28} ink={ink} />
              )}
              <EndpointNode
                type={lane.output}
                ink={ink}
                skeleton={skeleton}
                size={size}
              />
            </div>
          ))}
        </div>
      </div>
    );
  }

  const kids: ReactNode[] = [];
  if (graph.inputs) {
    kids.push(
      <JoinHead
        key="inputs"
        inputs={graph.inputs}
        ink={ink}
        size={size}
        skeleton={skeleton}
        laneGap={laneGap}
        renderRow={row}
      />,
    );
  } else {
    kids.push(
      <EndpointNode
        key="src"
        type={graph.inputType ?? "guitar"}
        ink={ink}
        skeleton={skeleton}
        size={size}
      />,
    );
  }

  graph.stages.forEach((st, i) => {
    kids.push(<Wire key={`sw${String(i)}`} ink={ink} />);
    if (st.kind === "split") {
      kids.push(splitSection(st, `st${String(i)}`));
    } else {
      st.blocks.forEach((b, j) => {
        if (j > 0)
          kids.push(<Wire key={`bw${String(i)}-${String(j)}`} ink={ink} />);
        kids.push(tile(b, `b${String(i)}-${String(j)}`));
      });
    }
  });

  if (graph.outputs) {
    kids.push(<Wire key="wfork" ink={ink} />);
    kids.push(
      <ForkTail
        key="outputs"
        outputs={graph.outputs}
        ink={ink}
        size={size}
        skeleton={skeleton}
        laneGap={laneGap}
        renderRow={row}
      />,
    );
  } else {
    kids.push(<Wire key="wout" ink={ink} />);
    kids.push(
      <EndpointNode
        key="sink"
        type={graph.outputType ?? "out"}
        ink={ink}
        skeleton={skeleton}
        size={size}
      />,
    );
  }

  return (
    <div style={stripScroll} className="tmp-block-strip">
      <div style={stripStyle}>{kids}</div>
    </div>
  );
}

export const SignalChainView = memo(SignalChainViewImpl);
