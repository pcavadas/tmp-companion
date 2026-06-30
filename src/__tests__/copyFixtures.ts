// Shared Copy-journey test fixtures: the 3-block series preset + its backup row, used by
// every test that drives the real Copy flow through a mocked invoke bridge (the full happy
// path in CopyHappyPath.test.tsx and the BUG-2 hero-patch lock in hardeningFixes.test.tsx).
// The captions ("DYNCMP"/"65TWN"/"SM-HALL") and origin chip names ("DYNAMIC COMPRESSOR" /
// "FENDER '65 TWIN REVERB" / "SMALL HALL REVERB") come from the real blockArt catalog, so
// the UI resolves them faithfully.

import type { ActiveGraph, GraphNode } from "../lib/types";

export const graphNode = (
  group: string,
  id: string,
  model: string,
): GraphNode => ({
  group_id: group,
  node_id: id,
  model,
  bypassed: false,
});

/** A real 3-block series preset (DynaComp → '65 Twin → Small Hall). `slot` is null for the
 *  generic case; pass a list index when a test needs the graph to resolve to the active slot. */
export function seriesGraph(
  name: string,
  slot: number | null = null,
): ActiveGraph {
  const nodes = [
    graphNode("G1", "n1", "ACD_DynaComp"),
    graphNode("G1", "n2", "ACD_TwinReverb65NoFx"),
    graphNode("G1", "n3", "ACD_TMSmallHall"),
  ];
  return {
    name,
    slot,
    template: "gtrSeries",
    split_mix: null,
    nodes,
    stages: [{ kind: "series", blocks: nodes }],
  };
}

/** One `read_library_via_backup` preset row. The backup keys each row at the 1-based DEVICE
 *  slot (listIndex + 1) — useCopyLibrary maps slot−1 back. */
export function backupRow(listIndex: number, name: string) {
  const g = seriesGraph(name);
  return {
    slot: listIndex + 1,
    name,
    scene_count: 1,
    scenes: [{ name: "Base", fs: null }],
    amp_candidates: [],
    blocks: g.nodes.map((n) => ({
      group_id: n.group_id,
      node_id: n.node_id,
      fender_id: n.model,
    })),
    graph: g,
    footswitches: [],
  };
}
