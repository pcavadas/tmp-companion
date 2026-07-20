// src/__tests__/MatchCard.test.tsx — the "Match reference" card's reuse-vs-insert
// gate: an existing non-bypassed EQ-10 in the chain gets value-aware param moves
// (no CPU change), never a second stacked EQ-10; only a chain with none falls
// back to inserting one. doctorApply is mocked (PrescriptionCard's apply path).

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { ThemeProvider } from "../theme/ThemeProvider";

vi.mock("../lib/invoke", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/invoke")>();
  return {
    ...actual,
    doctorApply: vi.fn(),
  };
});

// Imported AFTER the mock so MatchCard/PrescriptionCard pick it up.
import { doctorApply } from "../lib/invoke";
import { MatchCard } from "../views/doctor/MatchCard";
import type { DoctorSoundResult, GraphNode } from "../lib/types";

const LABELS = ["Lows", "Low-mids", "Mids", "High-mids", "Highs", "Air"];

function sound(balanceDb: number[]): DoctorSoundResult {
  return {
    key: "p0",
    listIndex: 0,
    scene: null,
    footswitch: null,
    label: "This sound",
    tag: null,
    diags: [],
    integratedLufs: -20,
    tailRatioDb: 0,
    balanceDb,
    bandLabels: LABELS,
    cutThrough: null,
    error: null,
  };
}

const AMP_NODE: GraphNode = {
  group_id: "G1",
  node_id: "amp1",
  model: "ACD_TweedDeluxe",
  bypassed: false,
  params: {},
};

// Only Lows carries a delta (3 dB, ref − sound) → the one surviving move,
// log-nearest EQ-10 band gain62hz (see matchModel.test.ts's derivation notes).
const referenceSound = sound([3, 0, 0, 0, 0, 0]);
const thisSound = sound([0, 0, 0, 0, 0, 0]);

function renderCard(nodes: GraphNode[]) {
  return render(
    <ThemeProvider>
      <MatchCard
        sound={thisSound}
        reference={referenceSound}
        listIndex={0}
        presetName="Test Preset"
        nodes={nodes}
        footswitches={[]}
      />
    </ThemeProvider>,
  );
}

beforeEach(() => {
  vi.mocked(doctorApply).mockReset();
  vi.mocked(doctorApply).mockResolvedValue({
    beforeClip: "data:audio/wav;base64,AAAA",
    afterClip: "data:audio/wav;base64,BBBB",
  });
});

describe("MatchCard — reuse vs insert", () => {
  it("emits a param op against the existing EQ-10, not an insert_node, when one is in the chain", async () => {
    const user = userEvent.setup();
    const eqNode: GraphNode = {
      group_id: "G1",
      node_id: "eq1",
      model: "ACD_TenBandEQStereo",
      bypassed: false,
      params: { gain62hz: 4 },
    };
    renderCard([AMP_NODE, eqNode]);

    expect(screen.getByText("One-click fix")).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: /apply to the unit/i }),
    );

    expect(doctorApply).toHaveBeenCalledWith(
      expect.objectContaining({
        ops: [
          {
            kind: "param",
            groupId: "G1",
            nodeId: "eq1",
            param: "gain62hz",
            value: 7, // current 4 + move 3, well inside +/-12
          },
        ],
      }),
    );
  });

  it("ignores a bypassed EQ-10 and still inserts a new one", async () => {
    const user = userEvent.setup();
    const bypassedEq: GraphNode = {
      group_id: "G1",
      node_id: "eq1",
      model: "ACD_TenBandEQStereo",
      bypassed: true,
      params: { gain62hz: 4 },
    };
    renderCard([AMP_NODE, bypassedEq]);

    expect(screen.getByText("Rebuilds the chain")).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: /apply to the unit/i }),
    );

    expect(doctorApply).toHaveBeenCalledWith(
      expect.objectContaining({
        ops: [
          {
            kind: "insert_node",
            groupId: "G1",
            beforeFenderId: null,
            fenderId: "ACD_TenBandEQStereo",
            params: [["gain62hz", 3]],
          },
        ],
      }),
    );
  });

  it("inserts a new EQ-10 when the chain has none", async () => {
    const user = userEvent.setup();
    renderCard([AMP_NODE]);

    expect(screen.getByText("Rebuilds the chain")).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: /apply to the unit/i }),
    );

    expect(doctorApply).toHaveBeenCalledWith(
      expect.objectContaining({
        ops: [
          {
            kind: "insert_node",
            groupId: "G1",
            beforeFenderId: null,
            fenderId: "ACD_TenBandEQStereo",
            params: [["gain62hz", 3]],
          },
        ],
      }),
    );
  });
});
