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
// No "intelligence": no by-role/this-model auto-matching. The user chooses a block on the
// reference's strip, then explicitly places it — a "+" gap in a target (Insert before /
// after a block) or a target block's menu (Replace) — or removes a target block. `change` drives the edit badge ("replaced" → ⟳, "added" → +) and the
// copied-in styling; `nodeId === null` marks an inserted (not-yet-on-device) block.

import { chargedTotal } from "../../models/cpu";
import type {
  ActiveGraph,
  CopyAnchor,
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
  /** The device's bypass state. Carried so Copy's strips draw a bypassed block the
   *  way the Level hero does, and so the optimistic cache patch after a save keeps
   *  every untouched block's state. A replaced or inserted block is a new device
   *  node built from the model id alone (no params ride the op), so it is drawn
   *  active (`false`); the device read-back, when adopted, carries its real state. */
  bypassed: boolean;
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

/** How a chosen model lands on its anchor block: swap it, or insert before/after it. */
export type EditMode = "replace" | "before" | "after";

/** The working-copy map for the chosen targets — keyed by slot. A slot may be absent
 *  (a target outside the current set), so indexing yields `PresetEdit | undefined`. */
export type EditMap = Partial<Record<number, PresetEdit>>;

const EMPTY_EDIT_GRAPH: EditGraph = {
  inputType: null,
  outputType: null,
  inputs: null,
  outputs: null,
  lanes: null,
  stages: [],
};

// ── uid minting (pure — scoped to one derivation, never a module counter) ─────
// A uid is `cb<N>`. It keys React rows, the chosen block and the open block menu, and an INSERTED
// block's uid is its insert key on the wire (`diffToOps`) — which a later round's
// optimistic read-back also adopts as that block's synthetic node id
// (`activeFromEditGraph`). So a fresh uid must clear every `cb<N>` already in the
// graph: its uids AND its node ids. Each derivation mints from its own counter, so
// the same input always yields the same uids — safe inside a render-phase `useMemo`
// or a state updater (which StrictMode may call twice).

const UID_RE = /^cb(\d+)$/;

/** The `<N>` of a `cb<N>` id, or 0 for any other id (a real device node id). */
function uidNum(id: string | null | undefined): number {
  const m = id == null ? null : UID_RE.exec(id);
  return m ? Number(m[1]) : 0;
}

/** A uid minter whose first uid is `cb<floor + 1>`. */
function uidMinter(floor: number): () => string {
  let n = floor;
  return () => {
    n += 1;
    return `cb${String(n)}`;
  };
}

// ── graph derivation from the device's routed graph ───────────────────────────
// Each device node already knows its group + node id; the routed `stages` /
// `inputs` / `outputs` / `lanes` views tell us the SERIES vs PARALLEL shape. We
// never alter the routing — only the blocks within it.

function toEditBlock(n: GraphNode, newUid: () => string): EditBlock {
  return {
    uid: newUid(),
    group: n.group_id,
    nodeId: n.node_id,
    model: n.model,
    change: null,
    bypassed: n.bypassed,
    cabSimId: n.cab_sim_id,
    cabSimId2: n.cab_sim_id2,
    cabSim2Enabled: n.cab_sim2_enabled,
  };
}

/** Build the editable `EditGraph` from the device's `ActiveGraph`, minting a uid per
 *  block and preserving the real routing (inline splits, dual lanes, merge/fork). */
export function editGraphFromActive(graph: ActiveGraph): EditGraph {
  const newUid = uidMinter(
    graph.nodes.reduce((m, n) => Math.max(m, uidNum(n.node_id)), 0),
  );
  const toBlock = (n: GraphNode) => toEditBlock(n, newUid);
  const mkLane = <T extends string>(l: { type: T; blocks: GraphNode[] }) => ({
    type: l.type,
    blocks: l.blocks.map(toBlock),
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
          blocks: l.blocks.map(toBlock),
        }))
      : null;
  const stages: EditStage[] = graph.stages.map((st) =>
    st.kind === "split"
      ? {
          kind: "split",
          uid: newUid(),
          a: st.a.map(toBlock),
          b: st.b.map(toBlock),
        }
      : { kind: "series", blocks: st.blocks.map(toBlock) },
  );

  const hasRoute =
    stages.length > 0 || inputs != null || outputs != null || lanes != null;
  // No routed structure (truncated read) — fall back to one flat series stage.
  const finalStages: EditStage[] = hasRoute
    ? stages
    : [{ kind: "series", blocks: graph.nodes.map(toBlock) }];

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
    bypassed: b.bypassed,
    params: {}, // the edit model doesn't carry params; the optimistic cache doesn't need them
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

/** The highest `cb<N>` in a working copy — over its block uids, its split uids, the
 *  node ids it holds, and the original node ids (a removed block's id included). */
function maxUidNum(edit: PresetEdit): number {
  let max = 0;
  for (const st of edit.graph.stages) {
    if (st.kind === "split") max = Math.max(max, uidNum(st.uid));
  }
  eachBlock(edit.graph, (b) => {
    max = Math.max(max, uidNum(b.uid), uidNum(b.nodeId));
  });
  for (const id of edit.origByNodeId.keys()) max = Math.max(max, uidNum(id));
  return max;
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

/** The Copy step-2 undo/redo history: a snapshot stack of the whole staged edit plus a
 *  pointer (the working copy is `stack[idx]`), and the library graph each slot's edit
 *  was built from. */
export interface EditHistory {
  stack: EditMap[];
  idx: number;
  /** slot → the `ActiveGraph` its working copy was derived from (identity-compared:
   *  a re-scan or a save's cache patch hands the slot a new object). */
  bases: Map<number, ActiveGraph | null>;
}

/** Re-target a history onto the slots now selected, keeping every staged edit (and its
 *  undo/redo steps) that is still valid. A slot keeps its edit when it was already in
 *  the history, is not in `reset`, and its library graph is the very object the edit was
 *  built from; any other slot gets a fresh working copy. Deselected slots drop out of
 *  every snapshot, and the steps that only touched them collapse, so undo never lands on
 *  a no-op step. With no prior history this is a fresh one-step history. */
export function retargetHistory(
  prev: EditHistory | null,
  slots: number[],
  graphForSlot: (slot: number) => ActiveGraph | null,
  reset: ReadonlySet<number> = new Set(),
): EditHistory {
  const bases = new Map(slots.map((s) => [s, graphForSlot(s)] as const));
  const keep = prev
    ? slots.filter(
        (s) =>
          !reset.has(s) &&
          prev.bases.has(s) &&
          prev.bases.get(s) === bases.get(s),
      )
    : [];
  const fresh = initEdit(
    slots.filter((s) => !keep.includes(s)),
    graphForSlot,
  );
  if (!prev) return { stack: [fresh], idx: 0, bases };

  const project = (snap: EditMap): EditMap => {
    const out: EditMap = { ...fresh };
    for (const s of keep) out[s] = snap[s];
    return out;
  };
  // A step is a no-op once projected when every kept slot's edit is the same object as
  // in the step before (a commit copies the map and replaces only the slot it edited).
  const same = (a: EditMap, b: EditMap): boolean =>
    keep.every((s) => a[s] === b[s]);
  const stack: EditMap[] = [];
  let idx = 0;
  prev.stack.forEach((snap, i) => {
    const p = project(snap);
    const last = stack.length > 0 ? stack[stack.length - 1] : null;
    if (!last || !same(last, p)) stack.push(p);
    if (i === prev.idx) idx = stack.length - 1;
  });
  return { stack, idx, bases };
}

// ── derived helpers ──────────────────────────────────────────────────────────

/** Has this preset been edited? True iff it would produce device ops — a replace
 *  (incl. a same-model replace, which copies the reference block's settings), an
 *  inserted block, or a removed original. The single source of truth shared with the
 *  badge (`change`) and the save (`diffToOps`). */
export function isEdited(edit: PresetEdit): boolean {
  return diffToOps(edit).length > 0;
}

/** Total DSP cost of the edited graph, as the device's budget check charges it
 *  (an uncosted id counts `UNKNOWN_CPU`). */
export function cpuOfGraph(graph: EditGraph): number {
  const models: string[] = [];
  eachBlock(graph, (b) => models.push(b.model));
  return chargedTotal(models);
}

/** Find an edit block by uid (a chosen source block, an open block menu), descending
 *  into splits. */
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
  mode: EditMode,
  model: string,
): PresetEdit {
  const next = cloneEdit(edit);
  const loc = locate(next.graph, uid);
  if (!loc) return next;
  const { arr, i } = loc;
  const anchor = arr[i];
  const newUid = uidMinter(maxUidNum(edit));
  if (mode === "replace") {
    // A replace is always an edit — even a same-model pick copies the reference block's
    // settings onto the target. An inserted block (no nodeId) stays "added" since it's
    // net-new; an original block becomes "replaced".
    const change: Change = anchor.nodeId == null ? "added" : "replaced";
    arr[i] = { ...anchor, model, change, bypassed: false };
  } else {
    const inserted: EditBlock = {
      uid: newUid(),
      group: anchor.group,
      nodeId: null,
      model,
      change: "added",
      bypassed: false,
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
// then inserts — each anchor names a block by its original nodeId or an earlier insert's
// key, and the backend resolves it to the id the device holds now (a replace or insert
// mints one: the FenderId, suffixed `_1`, `_2`… when the group already uses
// it, fw 1.8.58 static RE). The right-to-left emit + remove-then-insert order is what
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
    // the in-array successor may belong to a different group; fw 1.8.58 aborts on an
    // anchor its group lacks. So skip past other-group blocks to the next SAME-group
    // successor; none after it → append at the group end (before = null). The successor
    // is named by its original nodeId, or — when it is itself new, and so already
    // inserted by this right-to-left walk — by its key; the backend maps either to the
    // device's current id.
    for (let i = arr.length - 1; i >= 0; i--) {
      const b = arr[i];
      if (b.nodeId != null) continue; // original/surviving tile — not an insert
      let before: CopyAnchor | null = null;
      for (let j = i + 1; j < arr.length; j++) {
        const nx = arr[j];
        if (nx.group === b.group) {
          before =
            nx.nodeId != null
              ? { kind: "node", nodeId: nx.nodeId }
              : { kind: "inserted", key: nx.uid };
          break;
        }
      }
      inserts.push({
        kind: "insert",
        group: b.group,
        key: b.uid,
        before,
        repl: { kind: "model", fenderId: b.model },
      });
    }
  }

  return [...removes, ...replaces, ...inserts];
}

// ── post-save read-back reconciliation ───────────────────────────────────────

/** Per-group ordered model roster. Node ids are deliberately NOT part of it — a device
 *  replace re-assigns them and an insert mints one — and it is keyed per group because the
 *  optimistic graph lists `nodes` in signal order while a device graph lists them in sorted
 *  group order, so a flat compare would mismatch any chain that spans groups. */
export function groupRoster(graph: ActiveGraph): Map<string, string[]> {
  const out = new Map<string, string[]>();
  for (const n of graph.nodes) {
    const models = out.get(n.group_id);
    if (models) models.push(n.model);
    else out.set(n.group_id, [n.model]);
  }
  return out;
}

/** True when both graphs carry the same blocks per group, as a MULTISET — within-group
 *  order is not compared: the unit accepts several blocks of one model in a group, and
 *  with a node id being its FenderId an insert anchored on a duplicated model lands where
 *  the device decides (online 2026-09-03: four TubeScreamers in one group, the device's
 *  order ≠ the projected one). A multiset match still proves the read is the post-edit
 *  document (a stale one has different blocks, a partial fewer), and adopting it hands the
 *  cache the device's real order. */
export function sameRoster(a: ActiveGraph, b: ActiveGraph): boolean {
  const ra = groupRoster(a);
  const rb = groupRoster(b);
  if (ra.size !== rb.size) return false;
  for (const [group, models] of ra) {
    const other = rb.get(group);
    if (other?.length !== models.length) return false;
    const sa = [...models].sort();
    const sb = [...other].sort();
    if (sa.some((m, i) => m !== sb[i])) return false;
  }
  return true;
}

/** The graph the Copy save patches the library cache with. `optimistic` is rebuilt from the
 *  edit the device acked (`activeFromEditGraph`) and is the default; `readBack` — the
 *  device's working copy, re-prompted by `copy_apply` after the last confirmed op and
 *  BEFORE the save — is adopted ONLY when it shows exactly those blocks (it then contributes
 *  the device's real node ids and params, which a later edit of an inserted block needs). A
 *  read-back that disagrees is never the truth: the earlier buffer-scraped one (2026-09-02,
 *  fw 1.8.45) was the LOAD-TIME graph, and preferring it patched the cache back to the
 *  pre-edit blocks. Whether a read-back is verifiable at all is the backend's call
 *  (`copy_apply` omits `graph` for an edit the roster cannot see, e.g. a same-model
 *  re-stamp) — this only checks the one it was handed. */
export function reconcileReadBack(
  optimistic: ActiveGraph,
  readBack: ActiveGraph | undefined,
): ActiveGraph {
  return readBack && sameRoster(optimistic, readBack) ? readBack : optimistic;
}
