import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";

import { SignalChainView, type StripGraph } from "../views/SignalChainView";

const b = (name: string) => ({ name });
const text = (graph: StripGraph) =>
  render(<SignalChainView graph={graph} />).container.textContent;

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
