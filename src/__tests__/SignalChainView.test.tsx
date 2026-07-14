import { describe, expect, it } from "vitest";
import { render, within } from "@testing-library/react";

import { SignalChainView, type StripGraph } from "../views/SignalChainView";
import { ThemeProvider } from "../theme/ThemeProvider";

const b = (name: string) => ({ name });
const text = (graph: StripGraph) =>
  render(
    <ThemeProvider>
      <SignalChainView graph={graph} />
    </ThemeProvider>,
  ).container.textContent;

function parentOf(el: HTMLElement): HTMLElement {
  const p = el.parentElement;
  if (!p) throw new Error("expected a parent element");
  return p;
}

// Walk from a leaf caption <span> (a block name, or an OUT/GUITAR/MIC label) up
// to its lane-row container: span -> its label-row div -> the node's own root
// div (BlockTile/EndpointNode) -> the lane row rendered by ForkTail/JoinHead/
// the `lanes` case. Coupled to that 3-level nesting in SignalChainView.tsx —
// bump the hop count if BlockTile/EndpointNode's wrapper nesting changes.
const laneRowOf = (el: HTMLElement): HTMLElement =>
  parentOf(parentOf(parentOf(el)));

describe("SignalChainView — path-template grammar", () => {
  it.each([
    [
      "gtrSeries",
      {
        template: "gtrSeries",
        inputType: "guitar",
        outputType: "out",
        stages: [{ kind: "series", blocks: [b("G1"), b("G2")] }],
      } satisfies StripGraph,
      ["GUITAR", "G1", "G2", "OUT"],
      ["JOIN", "OUT 1", "OUT 2"],
    ],
    [
      "gtrParallel1",
      {
        template: "gtrParallel1",
        stages: [
          { kind: "series", blocks: [b("G1")] },
          { kind: "split", a: [b("G2")], b: [b("G3")] },
          { kind: "series", blocks: [b("G4")] },
        ],
      } satisfies StripGraph,
      ["GUITAR", "SPLIT", "MIX", "G4", "OUT"],
      ["JOIN", "OUT 1", "OUT 2"],
    ],
    [
      "gtrParallel2",
      {
        template: "gtrParallel2",
        stages: [
          { kind: "split", a: [b("G2")], b: [b("G3")] },
          { kind: "split", a: [b("G5")], b: [b("G6")] },
        ],
      } satisfies StripGraph,
      ["GUITAR", "SPLIT", "MIX", "OUT"],
      ["JOIN", "OUT 1", "OUT 2"],
    ],
    [
      "micSeries",
      {
        template: "micSeries",
        inputType: "mic",
        stages: [{ kind: "series", blocks: [b("M1")] }],
      } satisfies StripGraph,
      ["MIC/LINE", "M1", "OUT"],
      ["GUITAR", "JOIN", "OUT 1", "OUT 2"],
    ],
    [
      "micParallel1",
      {
        template: "micParallel1",
        inputType: "mic",
        stages: [
          { kind: "series", blocks: [b("M1")] },
          { kind: "split", a: [b("M2")], b: [b("M3")] },
        ],
      } satisfies StripGraph,
      ["MIC/LINE", "SPLIT", "MIX", "OUT"],
      ["GUITAR", "JOIN", "OUT 1", "OUT 2"],
    ],
    [
      "gtrMicSeries",
      {
        template: "gtrMicSeries",
        inputs: {
          a: { type: "guitar", blocks: [] },
          b: { type: "mic", blocks: [] },
        },
        stages: [{ kind: "series", blocks: [b("G1"), b("M1")] }],
      } satisfies StripGraph,
      ["GUITAR", "MIC/LINE", "JOIN", "G1", "M1", "OUT"],
      ["OUT 1", "OUT 2"],
    ],
    [
      "gtrMicMix",
      {
        template: "gtrMicMix",
        inputs: {
          a: { type: "guitar", blocks: [b("G1")] },
          b: { type: "mic", blocks: [b("M1")] },
        },
        stages: [],
      } satisfies StripGraph,
      ["GUITAR", "MIC/LINE", "JOIN", "G1", "M1", "OUT"],
      ["OUT 1", "OUT 2"],
    ],
    [
      "gtrMicMix2",
      {
        template: "gtrMicMix2",
        inputs: {
          a: { type: "guitar", blocks: [b("G1"), b("G2")] },
          b: { type: "mic", blocks: [b("M1")] },
        },
        stages: [],
      } satisfies StripGraph,
      ["GUITAR", "MIC/LINE", "JOIN", "G2", "OUT"],
      ["OUT 1", "OUT 2"],
    ],
    [
      "gtrMicMix3",
      {
        template: "gtrMicMix3",
        inputs: {
          a: { type: "guitar", blocks: [b("G1")] },
          b: { type: "mic", blocks: [b("M1"), b("M2")] },
        },
        stages: [],
      } satisfies StripGraph,
      ["GUITAR", "MIC/LINE", "JOIN", "M2", "OUT"],
      ["OUT 1", "OUT 2"],
    ],
    [
      "gtrMicParallel",
      {
        template: "gtrMicParallel",
        lanes: [
          { input: "guitar", output: "out1", blocks: [b("G1")] },
          { input: "mic", output: "out2", blocks: [b("M1")] },
        ],
        stages: [],
      } satisfies StripGraph,
      ["GUITAR", "MIC/LINE", "G1", "M1", "OUT 1", "OUT 2"],
      ["JOIN"],
    ],
    [
      "gtrSplit",
      {
        template: "gtrSplit",
        stages: [{ kind: "series", blocks: [b("G1")] }],
        outputs: {
          a: { type: "out1", blocks: [b("G2")] },
          b: { type: "out2", blocks: [b("G5")] },
        },
      } satisfies StripGraph,
      ["GUITAR", "G1", "SPLIT", "G2", "G5", "OUT 1", "OUT 2"],
      ["JOIN"],
    ],
    [
      "micSplit",
      {
        template: "micSplit",
        inputType: "mic",
        stages: [{ kind: "series", blocks: [b("M1")] }],
        outputs: {
          a: { type: "out1", blocks: [b("M2")] },
          b: { type: "out2", blocks: [b("M3")] },
        },
      } satisfies StripGraph,
      ["MIC/LINE", "M1", "SPLIT", "M2", "M3", "OUT 1", "OUT 2"],
      ["GUITAR", "JOIN"],
    ],
  ])("renders %s labels", (_name, graph, present, absent) => {
    const rendered = text(graph);
    for (const label of present) expect(rendered).toContain(label);
    for (const label of absent) expect(rendered).not.toContain(label);
  });

  it("keeps sequential split diamonds distinct", () => {
    const rendered = text({
      template: "gtrParallel2",
      stages: [
        { kind: "split", a: [b("A")], b: [b("B")] },
        { kind: "split", a: [b("C")], b: [b("D")] },
      ],
    });
    expect(rendered.split("SPLIT").length - 1).toBe(2);
    expect(rendered.split("MIX").length - 1).toBe(2);
  });
});

// The label-presence tests above only prove a name appears somewhere in the
// document — they would NOT catch a lane-swap bug (a block rendered under the
// wrong OUT/lane), because both labels stay present in the text regardless of
// which parent they're under. That's exactly the bug class PR #78 fixed
// (gtrSplit/micSplit lanes bunched onto the wrong side). These assert the
// block actually lives inside its OWN lane row, not just in the document.
describe("SignalChainView — lane-mapping DOM structure", () => {
  it("gtrSplit: out1/out2 lanes only contain their own group's blocks", () => {
    const { container } = render(
      <ThemeProvider>
        <SignalChainView
          graph={{
            template: "gtrSplit",
            stages: [{ kind: "series", blocks: [b("G1")] }],
            outputs: {
              a: { type: "out1", blocks: [b("G2"), b("G3")] },
              b: { type: "out2", blocks: [b("G5")] },
            },
          }}
        />
      </ThemeProvider>,
    );
    const out1Row = laneRowOf(within(container).getByText("OUT 1"));
    const out2Row = laneRowOf(within(container).getByText("OUT 2"));
    expect(within(out1Row).getByText("G2")).toBeInTheDocument();
    expect(within(out1Row).getByText("G3")).toBeInTheDocument();
    expect(within(out1Row).queryByText("G5")).toBeNull();
    expect(within(out2Row).getByText("G5")).toBeInTheDocument();
    expect(within(out2Row).queryByText("G2")).toBeNull();
    expect(within(out2Row).queryByText("G3")).toBeNull();
  });

  it("micSplit: out1/out2 lanes only contain their own group's blocks", () => {
    const { container } = render(
      <ThemeProvider>
        <SignalChainView
          graph={{
            template: "micSplit",
            inputType: "mic",
            stages: [{ kind: "series", blocks: [b("M1")] }],
            outputs: {
              a: { type: "out1", blocks: [b("M2")] },
              b: { type: "out2", blocks: [b("M3a"), b("M3b")] },
            },
          }}
        />
      </ThemeProvider>,
    );
    const out1Row = laneRowOf(within(container).getByText("OUT 1"));
    const out2Row = laneRowOf(within(container).getByText("OUT 2"));
    expect(within(out1Row).getByText("M2")).toBeInTheDocument();
    expect(within(out1Row).queryByText("M3a")).toBeNull();
    expect(within(out1Row).queryByText("M3b")).toBeNull();
    expect(within(out2Row).getByText("M3a")).toBeInTheDocument();
    expect(within(out2Row).getByText("M3b")).toBeInTheDocument();
    expect(within(out2Row).queryByText("M2")).toBeNull();
  });

  it("gtrMicParallel: each independent rail pairs its own input/output/blocks", () => {
    const { container } = render(
      <ThemeProvider>
        <SignalChainView
          graph={{
            template: "gtrMicParallel",
            stages: [],
            // Deliberately out of canonical order (mic first) — the renderer
            // must not assume index-0-is-guitar.
            lanes: [
              { input: "mic", output: "out2", blocks: [b("M1")] },
              { input: "guitar", output: "out1", blocks: [b("G1")] },
            ],
          }}
        />
      </ThemeProvider>,
    );
    const micRow = laneRowOf(within(container).getByText("MIC/LINE"));
    const gtrRow = laneRowOf(within(container).getByText("GUITAR"));
    expect(within(micRow).getByText("M1")).toBeInTheDocument();
    expect(within(micRow).getByText("OUT 2")).toBeInTheDocument();
    expect(within(micRow).queryByText("G1")).toBeNull();
    expect(within(gtrRow).getByText("G1")).toBeInTheDocument();
    expect(within(gtrRow).getByText("OUT 1")).toBeInTheDocument();
    expect(within(gtrRow).queryByText("M1")).toBeNull();
  });

  it("JoinHead (gtrMicSeries): input lanes render in inputs.a/b order, not a hardcoded kind", () => {
    const { container } = render(
      <ThemeProvider>
        <SignalChainView
          graph={{
            template: "gtrMicSeries",
            // Swapped vs. the backend's guitar-first convention — the frontend
            // must not hardcode which side is which either.
            inputs: {
              a: { type: "mic", blocks: [] },
              b: { type: "guitar", blocks: [] },
            },
            stages: [{ kind: "series", blocks: [b("M1"), b("G1")] }],
          }}
        />
      </ThemeProvider>,
    );
    const micRow = laneRowOf(within(container).getByText("MIC/LINE"));
    const gtrRow = laneRowOf(within(container).getByText("GUITAR"));
    expect(micRow).not.toBe(gtrRow);
    expect(
      !!(
        micRow.compareDocumentPosition(gtrRow) &
        Node.DOCUMENT_POSITION_FOLLOWING
      ),
    ).toBe(true);
  });
});
