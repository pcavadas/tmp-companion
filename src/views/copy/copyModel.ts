// src/views/copy/copyModel.ts — the Copy feature's editable working-copy model.
//
// A target preset is edited as an `EditGraph` — the SAME routed shape the device
// reports (`ActiveGraph`: series/split `stages`, dual `inputs`/`outputs`, fully
// independent `lanes`), but with editable blocks. So Copy renders through the Level
// page's `SignalChainView` engine (one renderer, real topology — true inline
// series → SPLIT[ a ∥ b ] → MIX → series) and edits IN PLACE: each tile carries its
// real device addressing (`group` + `nodeId` + `model`) so a Replace / Insert / Remove
// maps straight onto a live structural edit (`session::{replace_node, insert_node,
// remove_node}` via `copy_apply`). The feature NEVER rewrites a target's topology —
// inserts and removes happen WITHIN the lanes (groups) the target already has,
// including inside a split's sub-lanes.
//
// No "intelligence": no by-role/this-model auto-matching. The user taps a target block
// and explicitly chooses an origin block + a placement (Replace / Insert before / after)
// or Remove. `change` drives the edit badge ("replaced" → ⟳, "added" → +) and the
// copied-in styling; `nodeId === null` marks an inserted (not-yet-on-device) block.

import { resolveBlockArt, shortFallback } from "../../models/blockArt";
import { cpuForBid } from "../../models/cpu";
import type {
  ActiveGraph,
  CopyOp,
  GraphNode,
  InputLane,
  OutputLane,
  Stage,
} from "../../lib/types";

export type SourceType = "guitar" | "mic";
export type SinkType = "out" | "out1" | "out2";

/** How a tile was changed by an edit. `replaced` = in-place model swap (⟳ badge);
 *  `added` = an inserted copied-in block (+ badge). */
export type Change = "added" | "replaced";

/** A block in the editable working copy. Original device blocks carry their real
 *  `nodeId`; an inserted (copied-in) block has `nodeId === null` until it's saved. */
export interface EditBlock {
  /** Stable handle for React keys + targeting a specific instance across edits. */
  uid: string;
  /** Device group key (e.g. "G1", "M1") the block lives in — the insert/replace/
   *  remove address space. */
  group: string;
  /** The device node id, or null for a not-yet-saved inserted block. */
  nodeId: string | null;
  /** Exact device model id (FenderId). */
  model: string;
  /** The edit that produced this tile, or null for an untouched original. */
  change: Change | null;
  /** Dual-cab CabSim params (display-only; carried so the Copy strip can name the
   *  cab + expand a dual-cab into two parallel tiles). NOT part of the edit/diff —
   *  `editBlockToNode` drops them. */
  cabSimId?: string;
  cabSimId2?: string;
  cabSim2Enabled?: boolean;
}

/** One ordered stage of a path: a `series` run, or a `split` with two parallel
 *  sub-lanes joined by a mix. Mirrors the device's `Stage`, with editable blocks. */
export type EditStage =
  | { kind: "series"; blocks: EditBlock[] }
  | { kind: "split"; uid: string; a: EditBlock[]; b: EditBlock[] };

export interface EditInputLane {
  type: SourceType;
  blocks: EditBlock[];
}

export interface EditOutputLane {
  type: Extract<SinkType, "out1" | "out2">;
  blocks: EditBlock[];
}

export interface EditIndependentLane {
  input: SourceType;
  output: Extract<SinkType, "out1" | "out2">;
  blocks: EditBlock[];
}

/** The editable routed graph of one target preset — isomorphic to `ActiveGraph`'s
 *  routing, with editable blocks. Renders through `SignalChainView` via `CopyPath`. */
export interface EditGraph {
  inputType: SourceType | null;
  outputType: SinkType | null;
  inputs: { a: EditInputLane; b: EditInputLane } | null;
  outputs: { a: EditOutputLane; b: EditOutputLane } | null;
  lanes: EditIndependentLane[] | null;
  stages: EditStage[];
}

/** The editable working copy of one target preset. */
export interface PresetEdit {
  graph: EditGraph;
  /** nodeId → its pristine { model, group }, captured at init. The model detects
   *  Replace ops; the group lets a Remove op address the right group after the node is
   *  spliced out of the graph (so it can't be read back off the lanes). */
  origByNodeId: Map<string, { model: string; group: string }>;
}

/** The working-copy map for the chosen targets — keyed by slot. A slot may be absent
 *  (a target outside the current set), so indexing yields `PresetEdit | undefined`. */
export type EditMap = Partial<Record<number, PresetEdit>>;

/** An origin palette entry — one distinct block from the reference preset. */
export interface OriginBlock {
  model: string;
  /** Full model name (chip label). */
  name: string;
  icon: string | undefined;
  tone: string | undefined;
  footswitch: "plate" | "metal" | "round" | undefined;
  /** ref-derived per-block body color (pedals) — overlays the tone default. */
  body: string | undefined;
  /** ref-derived control-panel color behind the knobs/sliders (EQ/filter pedals). */
  panel: string | undefined;
  /** Fender reverb cream-chassis accent (footswitch-band colour); the 8 reverbs. */
  accent: string | undefined;
  /** terse caption / 1.8 dispatch token (= art.short) the engine reads. */
  lab: string | undefined;
  /** Real DSP cost (% of budget), or null when the id is uncosted. */
  cpu: number | null;
}

const EMPTY_EDIT_GRAPH: EditGraph = {
  inputType: null,
  outputType: null,
  inputs: null,
  outputs: null,
  lanes: null,
  stages: [],
};

// ── uid minting (module-scoped, runtime only — never read during render) ──────
let _uid = 0;
function newUid(): string {
  _uid += 1;
  return `cb${String(_uid)}`;
}

// ── graph derivation from the device's routed graph ───────────────────────────
// Each device node already knows its group + node id; the routed `stages` /
// `inputs` / `outputs` / `lanes` views tell us the SERIES vs PARALLEL shape. We
// never alter the routing — only the blocks within it.

function toEditBlock(n: GraphNode): EditBlock {
  return {
    uid: newUid(),
    group: n.group_id,
    nodeId: n.node_id,
    model: n.model,
    change: null,
    cabSimId: n.cab_sim_id,
    cabSimId2: n.cab_sim_id2,
    cabSim2Enabled: n.cab_sim2_enabled,
  };
}

/** Build the editable `EditGraph` from the device's `ActiveGraph`, minting a uid per
 *  block and preserving the real routing (inline splits, dual lanes, merge/fork). */
export function editGraphFromActive(graph: ActiveGraph): EditGraph {
  const mkLane = <T extends string>(l: { type: T; blocks: GraphNode[] }) => ({
    type: l.type,
    blocks: l.blocks.map(toEditBlock),
  });
  const inputs = graph.inputs
    ? { a: mkLane(graph.inputs.a), b: mkLane(graph.inputs.b) }
    : null;
  const outputs = graph.outputs
    ? { a: mkLane(graph.outputs.a), b: mkLane(graph.outputs.b) }
    : null;
  const lanes =
    graph.lanes && graph.lanes.length > 0
      ? graph.lanes.map((l) => ({
          input: l.input,
          output: l.output,
          blocks: l.blocks.map(toEditBlock),
        }))
      : null;
  const stages: EditStage[] = graph.stages.map((st) =>
    st.kind === "split"
      ? {
          kind: "split",
          uid: newUid(),
          a: st.a.map(toEditBlock),
          b: st.b.map(toEditBlock),
        }
      : { kind: "series", blocks: st.blocks.map(toEditBlock) },
  );

  const hasRoute =
    stages.length > 0 || inputs != null || outputs != null || lanes != null;
  // No routed structure (truncated read) — fall back to one flat series stage.
  const finalStages: EditStage[] = hasRoute
    ? stages
    : [{ kind: "series", blocks: graph.nodes.map(toEditBlock) }];

  return {
    inputType: graph.input_type ?? null,
    outputType: graph.output_type ?? null,
    inputs,
    outputs,
    lanes,
    stages: finalStages,
  };
}

/** Inverse of {@link toEditBlock}. An inserted block (`nodeId === null`) gets a stable
 *  synthetic node id (its `uid`) so a SUBSEQUENT edit round treats it as an original
 *  block (re-editable / removable), matching the device after the save assigned one. */
function editBlockToNode(b: EditBlock): GraphNode {
  return {
    group_id: b.group,
    node_id: b.nodeId ?? b.uid,
    model: b.model,
    bypassed: false,
  };
}

/** Inverse of {@link editGraphFromActive}: rebuild the device-shaped `ActiveGraph` from
 *  an edited `EditGraph`, so the Copy save can patch the library cache OPTIMISTICALLY
 *  from the edit it just applied (no device read-back, no backup refetch). `base` carries
 *  the header fields the `EditGraph` doesn't model (`name`/`slot`/`template`/`split_mix`);
 *  the routing + blocks come from the edit. */
export function activeFromEditGraph(
  eg: EditGraph,
  base?: ActiveGraph,
): ActiveGraph {
  const inLane = (l: EditInputLane): InputLane => ({
    type: l.type,
    blocks: l.blocks.map(editBlockToNode),
  });
  const outLane = (l: EditOutputLane): OutputLane => ({
    type: l.type,
    blocks: l.blocks.map(editBlockToNode),
  });
  const inputs = eg.inputs
    ? { a: inLane(eg.inputs.a), b: inLane(eg.inputs.b) }
    : null;
  const outputs = eg.outputs
    ? { a: outLane(eg.outputs.a), b: outLane(eg.outputs.b) }
    : null;
  const lanes = eg.lanes
    ? eg.lanes.map((l) => ({
        input: l.input,
        output: l.output,
        blocks: l.blocks.map(editBlockToNode),
      }))
    : null;
  const stages: Stage[] = eg.stages.map((st) =>
    st.kind === "split"
      ? {
          kind: "split",
          a: st.a.map(editBlockToNode),
          b: st.b.map(editBlockToNode),
        }
      : { kind: "series", blocks: st.blocks.map(editBlockToNode) },
  );

  // Flat `nodes` list in signal order — the same walk `blockArrays` uses.
  const nodes: GraphNode[] = [];
  if (inputs) nodes.push(...inputs.a.blocks, ...inputs.b.blocks);
  for (const st of stages) {
    if (st.kind === "split") nodes.push(...st.a, ...st.b);
    else nodes.push(...st.blocks);
  }
  if (lanes) for (const l of lanes) nodes.push(...l.blocks);
  if (outputs) nodes.push(...outputs.a.blocks, ...outputs.b.blocks);

  return {
    name: base?.name ?? null,
    slot: base?.slot ?? null,
    template: base?.template ?? null,
    split_mix: base?.split_mix ?? null,
    nodes,
    input_type: eg.inputType,
    output_type: eg.outputType === "out" ? "out" : null,
    inputs,
    outputs,
    lanes,
    stages,
  };
}

// ── block-array walking (the address space the edit/diff helpers share) ───────
// Every block lives in exactly one array; the arrays appear in signal order so the
// diff's per-group insert anchoring lands correctly.

/** Every block array in the graph, in signal order (inputs → stages → lanes →
 *  outputs). Returns the LIVE arrays so a caller can splice/replace in place. */
export function blockArrays(graph: EditGraph): EditBlock[][] {
  const arrs: EditBlock[][] = [];
  if (graph.inputs) arrs.push(graph.inputs.a.blocks, graph.inputs.b.blocks);
  for (const st of graph.stages) {
    if (st.kind === "split") arrs.push(st.a, st.b);
    else arrs.push(st.blocks);
  }
  if (graph.lanes) for (const l of graph.lanes) arrs.push(l.blocks);
  if (graph.outputs) arrs.push(graph.outputs.a.blocks, graph.outputs.b.blocks);
  return arrs;
}

function eachBlock(graph: EditGraph, fn: (b: EditBlock) => void): void {
  for (const arr of blockArrays(graph)) for (const b of arr) fn(b);
}

/** Locate the array + index holding `uid` (descending into splits). */
function locate(
  graph: EditGraph,
  uid: string,
): { arr: EditBlock[]; i: number } | null {
  for (const arr of blockArrays(graph)) {
    const i = arr.findIndex((b) => b.uid === uid);
    if (i >= 0) return { arr, i };
  }
  return null;
}

// ── cloning (immutable updates) ───────────────────────────────────────────────

function cloneGraph(g: EditGraph): EditGraph {
  const cb = (b: EditBlock): EditBlock => ({ ...b });
  const cloneLane = <T extends string>(l: {
    type: T;
    blocks: EditBlock[];
  }) => ({
    type: l.type,
    blocks: l.blocks.map(cb),
  });
  return {
    inputType: g.inputType,
    outputType: g.outputType,
    inputs: g.inputs
      ? { a: cloneLane(g.inputs.a), b: cloneLane(g.inputs.b) }
      : null,
    outputs: g.outputs
      ? { a: cloneLane(g.outputs.a), b: cloneLane(g.outputs.b) }
      : null,
    lanes: g.lanes
      ? g.lanes.map((l) => ({
          input: l.input,
          output: l.output,
          blocks: l.blocks.map(cb),
        }))
      : null,
    stages: g.stages.map((st) =>
      st.kind === "split"
        ? { kind: "split", uid: st.uid, a: st.a.map(cb), b: st.b.map(cb) }
        : { kind: "series", blocks: st.blocks.map(cb) },
    ),
  };
}

/** Clone the editable graph (immutable updates); the original-node map is shared (never
 *  mutated). */
function cloneEdit(edit: PresetEdit): PresetEdit {
  return {
    graph: cloneGraph(edit.graph),
    origByNodeId: edit.origByNodeId,
  };
}

/** Build the editable working copy for a set of target presets keyed by slot. */
export function initEdit(
  slots: number[],
  graphForSlot: (slot: number) => ActiveGraph | null,
): Record<number, PresetEdit> {
  const out: Record<number, PresetEdit> = {};
  slots.forEach((s) => {
    const ag = graphForSlot(s);
    const graph = ag ? editGraphFromActive(ag) : cloneGraph(EMPTY_EDIT_GRAPH);
    const origByNodeId = new Map<string, { model: string; group: string }>();
    eachBlock(graph, (b) => {
      if (b.nodeId != null) {
        origByNodeId.set(b.nodeId, { model: b.model, group: b.group });
      }
    });
    out[s] = { graph, origByNodeId };
  });
  return out;
}

// ── derived helpers ──────────────────────────────────────────────────────────

/** Has this preset been edited? True iff it would produce device ops — a replace
 *  (incl. a same-model replace, which copies the reference block's settings), an
 *  inserted block, or a removed original. The single source of truth shared with the
 *  badge (`change`) and the save (`diffToOps`). */
export function isEdited(edit: PresetEdit): boolean {
  return diffToOps(edit).length > 0;
}

/** Total DSP cost of the edited graph (sum of every block's real cost). */
export function cpuOfGraph(graph: EditGraph): number {
  let sum = 0;
  eachBlock(graph, (b) => {
    sum += cpuForBid(b.model) ?? 0;
  });
  return Math.round(sum * 10) / 10;
}

/** The origin palette: every DISTINCT block in the reference preset, in path order. */
export function originBlocks(graph: ActiveGraph): OriginBlock[] {
  const seen = new Set<string>();
  const out: OriginBlock[] = [];
  graph.nodes.forEach((n) => {
    if (seen.has(n.model)) return;
    seen.add(n.model);
    const art = resolveBlockArt(n.model);
    out.push({
      model: n.model,
      name: art?.name ?? shortFallback(n.model),
      icon: art?.icon,
      tone: art?.tone,
      footswitch: art?.footswitch,
      body: art?.body,
      panel: art?.panel,
      accent: art?.accent,
      lab: art?.short,
      cpu: cpuForBid(n.model),
    });
  });
  return out;
}

/** Find an edit block by uid (for the open inline editor), descending into splits. */
export function findBlock(graph: EditGraph, uid: string): EditBlock | null {
  for (const arr of blockArrays(graph)) {
    const b = arr.find((x) => x.uid === uid);
    if (b) return b;
  }
  return null;
}

// ── edit reducers (pure, immutable) ──────────────────────────────────────────

/** Replace / insert-before / insert-after the block `uid` with `model`. */
export function applyEditOp(
  edit: PresetEdit,
  uid: string,
  mode: "replace" | "before" | "after",
  model: string,
): PresetEdit {
  const next = cloneEdit(edit);
  const loc = locate(next.graph, uid);
  if (!loc) return next;
  const { arr, i } = loc;
  const anchor = arr[i];
  if (mode === "replace") {
    // A replace is always an edit — even a same-model pick copies the reference block's
    // settings onto the target. An inserted block (no nodeId) stays "added" since it's
    // net-new; an original block becomes "replaced".
    const change: Change = anchor.nodeId == null ? "added" : "replaced";
    arr[i] = { ...anchor, model, change };
  } else {
    const inserted: EditBlock = {
      uid: newUid(),
      group: anchor.group,
      nodeId: null,
      model,
      change: "added",
    };
    arr.splice(mode === "before" ? i : i + 1, 0, inserted);
  }
  return next;
}

/** Remove the block `uid` from its lane (or split sub-lane). */
export function removeEditBlock(edit: PresetEdit, uid: string): PresetEdit {
  const next = cloneEdit(edit);
  const loc = locate(next.graph, uid);
  if (loc) loc.arr.splice(loc.i, 1);
  return next;
}

// ── diff → device ops (the save) ─────────────────────────────────────────────
// Walk the edited graph in signal order. For each ORIGINAL node still present that was
// replaced → Replace. For each ORIGINAL node now absent → Remove. For each INSERTED
// block → Insert anchored BEFORE the next block IN ITS OWN DEVICE GROUP. The device's
// `insertNode` field-2 means "insert BEFORE this node" (HW-verified, fw 1.8.45 — a
// short-anchor insert "before X" landed the new block ahead of X), field-2 must name a
// node in the SAME group as the insert, and OMITTING it appends at the group end. A
// visual series can span groups (amp in G1, pedals in G4), so the in-array successor may
// be in another group — we skip those and anchor on the next SAME-group successor, or
// append when the insert is last in its group (HW-verified: cross-group anchoring made
// the device drop the insert). Consecutive inserts are emitted RIGHT-TO-LEFT so each
// one's successor is already on the device when it anchors. Order: removes, replaces,
// then inserts — inserts anchor by the (post-replace) FenderId.
//
// DEVICE LIMITATION (no per-instance node identity): on the real unit a block's `nodeId`
// EQUALS its FenderId (model id) — there is no per-instance handle distinct from the
// model. So a single group can never hold two blocks of the SAME model (they'd be
// indistinguishable on the wire), and anchoring an insert by FenderId is sufficient and
// unambiguous. The earlier worry that bracketing a block with two same-model inserts
// could misplace them ("BUG-1") cannot arise — the duplicate-same-model-in-one-group
// state is unrepresentable. The right-to-left emit + remove-then-insert order is what
// makes the user's "insert A before B, insert C after B, remove B → exactly [A, C]"
// case correct (see copyModel.test.ts "INV-A"). See notes/write-safety.md.

export function diffToOps(edit: PresetEdit): CopyOp[] {
  const removes: CopyOp[] = [];
  const replaces: CopyOp[] = [];
  const inserts: CopyOp[] = [];

  // Which original nodeIds survive (present among edited tiles).
  const survivingNodeIds = new Set<string>();
  eachBlock(edit.graph, (b) => {
    if (b.nodeId != null) survivingNodeIds.add(b.nodeId);
  });

  // Removes: original nodes no longer present.
  edit.origByNodeId.forEach((orig, nodeId) => {
    if (!survivingNodeIds.has(nodeId)) {
      removes.push({ kind: "remove", group: orig.group, nodeId });
    }
  });

  for (const arr of blockArrays(edit.graph)) {
    // Replaces in signal order — any tile the user replaced (incl. a same-model pick,
    // which re-stamps the model; a Model op carries only the FenderId, not parameters).
    for (const b of arr) {
      if (b.nodeId != null && b.change === "replaced") {
        replaces.push({
          kind: "replace",
          group: b.group,
          nodeId: b.nodeId,
          repl: { kind: "model", fenderId: b.model },
        });
      }
    }
    // Inserts RIGHT-TO-LEFT: anchor each BEFORE the next block IN ITS OWN DEVICE GROUP.
    // `insertNode` field-2 (the "before" anchor) must name a node in the SAME group as the
    // insert — a visual series can span groups (e.g. an amp in G1 then pedals in G4), so
    // the in-array successor may belong to a different group; anchoring on it would name a
    // node absent from the insert's group and the device rejects it. So skip past
    // other-group blocks to the next SAME-group successor; none after it → append at the
    // group end (beforeFenderId = null).
    for (let i = arr.length - 1; i >= 0; i--) {
      const b = arr[i];
      if (b.nodeId != null) continue; // original/surviving tile — not an insert
      let beforeFenderId: string | null = null;
      for (let j = i + 1; j < arr.length; j++) {
        const nx = arr[j];
        if (nx.group === b.group) {
          beforeFenderId = nx.model;
          break;
        }
      }
      inserts.push({
        kind: "insert",
        group: b.group,
        beforeFenderId,
        repl: { kind: "model", fenderId: b.model },
      });
    }
  }

  return [...removes, ...replaces, ...inserts];
}
