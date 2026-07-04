import React, { useMemo } from "react";
import { useTheme } from "../theme/ThemeContext";
import { Icon } from "../ui/Icon";
import { Skel, SkelStatus } from "../ui/Skeleton";
import { Button } from "../ui/primitives";
import { nodeTileArt } from "../models/blockArt";
import { isComboBid } from "../models/catalog";
import { CPU_BUDGET, cpuStr, presetCpu } from "../models/cpu";
import { expandDualCab } from "./stripExpand";
import {
  SignalChainView,
  type StripBlock,
  type StripGraph,
  type StripIndependentLane,
  type StripInputLane,
  type StripOutputLane,
  type StripStage,
} from "./SignalChainView";
import { slotLabel } from "../lib/format";
import type { ActiveGraph, GraphNode, Stage } from "../lib/types";

/** Inline live-scene tag rendered next to the hero name. `neutral` → the loader-free
 * syncing em-dash (no `›`, faint tone, "catching up" tooltip); otherwise a `› {text}`
 * descriptor in the accent tone. */
export interface SceneTag {
  text: string;
  /** Token color for the tag text. */
  tone: string;
  /** Neutral syncing state — drop the `›` caret + show the catching-up tooltip. */
  neutral?: boolean;
}

export interface ActiveSignalChainViewProps {
  graph: ActiveGraph | null;
  /** The live active My-Presets index (0-based) for the slot badge. Authoritative
   *  over `graph.slot`, which is `null` on a live switch (field-3 carries no slot).
   *  Falls back to `graph.slot` at startup before the first live-preset event. */
  slot?: number | null;
  /** The active preset's identity (slot + name) is still arriving — ghost the
   *  slot badge + name as two skeleton bars. */
  presetLoading?: boolean;
  /** The slower signal-chain payload is still arriving — ghost the chain in
   *  place (independent of the identity). */
  diagramLoading?: boolean;
  /** The live scene next to the name (› BASE / › SCENE, or the neutral syncing —). */
  sceneTag?: SceneTag | null;
  /** The signal-view refresh failed while the recall landed — dim + grayscale the
   *  strip and overlay an amber "didn't refresh" chip with a Retry. The scene row
   *  already updated; only the picture is stale. */
  diagramError?: boolean;
  /** Retry the signal-view read (re-request the active graph). */
  onRetryDiagram?: () => void;
}

// A representative silhouette ghosted on the very first load, when no graph shape
// is known yet: endpoints + a series tile + one parallel split + a series tile.
// Captions are empty (the ghost hides them); once the real chain arrives it
// replaces this in place. When a prior graph IS known (e.g. swapping the active
// preset), that real shape is ghosted instead, so resolving causes zero reflow.
const SKELETON_GRAPH: StripGraph = {
  stages: [
    { kind: "series", blocks: [{ name: "" }] },
    { kind: "split", a: [{ name: "" }], b: [{ name: "" }] },
    { kind: "series", blocks: [{ name: "" }] },
  ],
};

// Resolve a device block's full art field set BY ID via the shared `nodeTileArt`
// helper (identical to the Catalog + Copy strip — never a degraded subset), then
// add the live bypass state. `nodeTileArt` branches on node kind: a standalone
// CabSim is NAMED from its cabinet (`cab_sim_id` → "British", not generic CAB IR)
// and carries the dual-cab params so `expandDualCab` splits it into two parallel
// tiles; a head-with-baked-cab / half-stack amp becomes a head-over-cab tile; a
// COMBO amp (form-driven `isComboBid`) stays a single combo tile — its built-in
// speaker is a cab_sim_id too, so without the flag it would wrongly stack.
// `cab_sim_id2`/`cab_sim2_enabled` are nulled backend-side (session.rs `is_cab_block`
// filter) for any non-CabSim node, so a combo never carries a second cab to split.
function toStripBlock(n: GraphNode): StripBlock {
  return {
    ...nodeTileArt(n.model, n.cab_sim_id, isComboBid(n.model)),
    model: n.model,
    bypassed: n.bypassed,
    cabSimId: n.cab_sim_id,
    cabSimId2: n.cab_sim_id2,
    cabSim2Enabled: n.cab_sim2_enabled,
  };
}

function toStripStage(st: Stage): StripStage {
  return st.kind === "split"
    ? { kind: "split", a: st.a.map(toStripBlock), b: st.b.map(toStripBlock) }
    : { kind: "series", blocks: st.blocks.map(toStripBlock) };
}

function toStripGraph(graph: ActiveGraph): StripGraph | null {
  // Expand any dual-cab CabSim node in a series stage into two parallel cab tiles
  // (matches Pro Control); single-cab / non-series cabs stay one named tile.
  const stages = expandDualCab(graph.stages.map(toStripStage));
  const inputs = graph.inputs
    ? {
        a: {
          type: graph.inputs.a.type,
          blocks: graph.inputs.a.blocks.map(toStripBlock),
        } satisfies StripInputLane,
        b: {
          type: graph.inputs.b.type,
          blocks: graph.inputs.b.blocks.map(toStripBlock),
        } satisfies StripInputLane,
      }
    : null;
  const outputs = graph.outputs
    ? {
        a: {
          type: graph.outputs.a.type,
          blocks: graph.outputs.a.blocks.map(toStripBlock),
        } satisfies StripOutputLane,
        b: {
          type: graph.outputs.b.type,
          blocks: graph.outputs.b.blocks.map(toStripBlock),
        } satisfies StripOutputLane,
      }
    : null;
  const lanes = graph.lanes
    ? graph.lanes.map((lane): StripIndependentLane => ({
        input: lane.input,
        output: lane.output,
        blocks: lane.blocks.map(toStripBlock),
      }))
    : null;
  const hasRoute =
    stages.length > 0 ||
    inputs != null ||
    outputs != null ||
    (lanes != null && lanes.length > 0);
  return hasRoute
    ? {
        template: graph.template,
        inputType: graph.input_type ?? undefined,
        outputType: graph.output_type ?? undefined,
        inputs,
        outputs,
        lanes,
        stages,
      }
    : null;
}

/** The hero's live DSP-load readout: the active preset's total CPU vs the device's
 *  per-preset cap (e.g. `44.3% / 76.5%`). The total turns warn-colored if it ever
 *  exceeds the cap. Sits opposite the slot/name. */
function HeroCpu({ cpu }: { cpu: number }) {
  const { t } = useTheme();
  const over = cpu > CPU_BUDGET;
  return (
    <span
      title={`CPU is capped at ${String(CPU_BUDGET)}% per preset on the Tone Master Pro.`}
      style={{
        display: "inline-flex",
        alignItems: "baseline",
        gap: 4,
        whiteSpace: "nowrap",
      }}
    >
      <span
        style={{
          fontFamily: t.mono,
          fontSize: t.fsData,
          fontWeight: 600,
          color: over ? t.warn : t.ink,
        }}
      >
        {cpuStr(cpu)}
      </span>
      <span style={{ fontFamily: t.mono, fontSize: t.fsData, color: t.faint }}>
        / {CPU_BUDGET}%
      </span>
      <span
        style={{
          fontFamily: t.mono,
          fontSize: t.fsTag,
          letterSpacing: t.lsTag,
          color: t.faint,
          textTransform: "uppercase",
        }}
      >
        CPU
      </span>
    </span>
  );
}

export function ActiveSignalChainView(props: ActiveSignalChainViewProps) {
  const {
    graph,
    slot = null,
    presetLoading = false,
    diagramLoading = false,
    sceneTag = null,
    diagramError = false,
    onRetryDiagram,
  } = props;
  const { t } = useTheme();
  const skeleton = presetLoading || diagramLoading;

  // Map the device's ordered stages once per graph change. Memoizing the whole
  // StripGraph object (not just the inner array) keeps the prop referentially
  // stable, so the memoized SignalChainView skips re-render on unrelated ticks.
  // (`toStripStage` is a module-level pure fn — safe to omit from deps.)
  const mappedGraph = useMemo<StripGraph | null>(
    () => (graph ? toStripGraph(graph) : null),
    [graph],
  );

  // Live DSP load of the active preset (sum of every block's real cost), shown in
  // the hero opposite the identity. Null until a graph is present.
  const cpu = useMemo(() => presetCpu(graph), [graph]);

  // ---- Block strip body ----
  let body: React.ReactNode;

  if (skeleton) {
    // Ghost the chain in place. If a prior graph's shape is known (swapping the
    // active preset) ghost THAT for zero reflow; otherwise a representative
    // silhouette so the chain's shape reads before the art arrives.
    const ghostGraph: StripGraph = mappedGraph ?? SKELETON_GRAPH;
    body = <SignalChainView graph={ghostGraph} size="md" skeleton />;
  } else if (!graph || !mappedGraph) {
    body = (
      <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
        <span
          style={{
            width: 48,
            height: 48,
            borderRadius: t.rLg,
            border: `0.5px solid ${t.hairlineStrong}`,
            flex: "0 0 auto",
          }}
        />
        <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
          <span
            style={{ fontFamily: t.serif, fontSize: t.fsName2, color: t.ink }}
          >
            No active preset
          </span>
          <span
            style={{
              fontFamily: t.mono,
              fontSize: t.fsData2,
              color: t.mutedInk,
            }}
          >
            load a preset on the amp to see its chain
          </span>
        </div>
      </div>
    );
  } else {
    // Map the device's ordered series/split stages to the shared SignalChainView,
    // resolving each block's real art by id. N sequential splits + series
    // segments render correctly; the routing is the device's, never positional.
    body = <SignalChainView graph={mappedGraph} size="md" />;
  }

  const slotForBadge = slot ?? graph?.slot ?? null;
  const slotBadge = slotForBadge != null ? slotLabel(slotForBadge) : "—";
  return (
    <div
      style={{
        background: t.bgAlt,
        borderBottom: `0.5px solid ${t.hairline}`,
        padding: "14px 18px 16px",
        boxSizing: "border-box",
      }}
    >
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          marginBottom: 12,
          gap: 16,
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 11,
            minWidth: 0,
          }}
        >
          {presetLoading ? (
            <>
              <Skel w={34} h={20} r={6} />
              <Skel w={150} h={15} r={5} />
            </>
          ) : (
            <>
              <span
                style={{
                  fontFamily: t.mono,
                  fontSize: t.fsLabel,
                  fontWeight: 500,
                  letterSpacing: "0.04em",
                  color: t.accentDeep,
                  border: `0.5px solid ${t.accentBorder}`,
                  background: t.accentSoft,
                  borderRadius: t.rBtn,
                  padding: "3px 9px",
                  flex: "0 0 auto",
                }}
              >
                {slotBadge}
              </span>
              <span
                style={{
                  fontFamily: t.serif,
                  fontSize: t.fsSubhead,
                  color: t.ink,
                  whiteSpace: "nowrap",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                }}
              >
                {graph?.name ?? "Current preset"}
              </span>
              {sceneTag && (
                <span
                  title={
                    sceneTag.neutral ? "Catching up to the unit…" : undefined
                  }
                  style={{
                    display: "inline-flex",
                    alignItems: "center",
                    gap: 6,
                    fontFamily: t.mono,
                    fontSize: t.fsData,
                    letterSpacing: t.lsTag,
                    color: sceneTag.tone,
                    whiteSpace: "nowrap",
                    flex: "0 0 auto",
                  }}
                >
                  {!sceneTag.neutral && (
                    <span style={{ color: t.faint, fontSize: 13 }}>{"›"}</span>
                  )}
                  {sceneTag.text}
                </span>
              )}
            </>
          )}
        </div>
        {/* While loading, the right side reads as work-in-progress (mono caption)
            rather than the CPU readout. "Loading signal chain…" only once the
            identity has resolved but the chain is still arriving. */}
        <div
          style={{
            flex: "0 0 auto",
            display: "flex",
            alignItems: "center",
          }}
        >
          {skeleton ? (
            <SkelStatus
              label={
                diagramLoading && !presetLoading
                  ? "Loading signal chain…"
                  : "Reading active preset…"
              }
            />
          ) : (
            cpu != null && <HeroCpu cpu={cpu} />
          )}
        </div>
      </div>
      {/* The diagram always redraws from the unit's REAL block state after a recall.
          That read can transiently fail (device congestion) — the scene row already
          updated, but this picture is stale: dim + grayscale it and offer a Retry. */}
      <div style={{ position: "relative" }}>
        <div
          style={{
            opacity: diagramError && !skeleton ? 0.32 : 1,
            filter: diagramError && !skeleton ? "grayscale(0.6)" : "none",
            transition: "opacity 0.2s, filter 0.2s",
            pointerEvents: diagramError ? "none" : "auto",
          }}
        >
          {body}
        </div>
        {diagramError && !skeleton && (
          <div
            style={{
              position: "absolute",
              inset: 0,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
            }}
          >
            <div
              style={{
                display: "inline-flex",
                alignItems: "center",
                gap: 10,
                padding: "8px 10px 8px 12px",
                borderRadius: t.rCard,
                background: t.bg,
                border: `0.5px solid ${t.sevWarnBorder}`,
                boxShadow: `0 8px 22px -10px ${t.shadow}`,
              }}
            >
              <Icon
                name="warn-tri"
                size={14}
                stroke={t.sevWarn}
                strokeWidth={1.6}
              />
              <span
                style={{
                  fontFamily: t.sans,
                  fontSize: t.fsUi,
                  color: t.ink2,
                  whiteSpace: "nowrap",
                }}
              >
                Scene recalled — signal view didn&apos;t refresh
              </span>
              <Button
                variant="ghost"
                small
                icon="refresh"
                onClick={onRetryDiagram}
              >
                Retry
              </Button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

export default ActiveSignalChainView;
