// src/views/copy/CopyPath.tsx — the Copy feature's signal-path renderer.
//
// Copy has NO renderer of its own: it reuses the Level page's `SignalChainView` engine
// (GUITAR/MIC input nodes, OUT nodes, SPLIT/MIX/JOIN diamonds, rounded split brackets,
// procedural per-model art). This adapter maps an `EditGraph` → the engine's `StripGraph`
// and renders it. Tiles are interactive only when `onTap` is provided (the editable
// target path); the read-only reference strip omits it, so its tiles render exactly as
// the Level page's do. `change` drives the +/⟲ badge; `selectedUid` the accent ring.

import { useMemo } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { nodeTileArt } from "../../models/blockArt";
import { isComboBid } from "../../models/catalog";
import {
  SignalChainView,
  type StripBlock,
  type StripGraph,
  type StripStage,
} from "../SignalChainView";
import { expandDualCab } from "../stripExpand";
import { type EditBlock, type EditGraph } from "./copyModel";

export interface CopyPathProps {
  graph: EditGraph;
  /** Tap handler — present only for the editable target path (makes tiles buttons). */
  onTap?: (uid: string) => void;
  /** The currently-open block's uid (drives the selected ring). */
  selectedUid?: string | null;
}

export function CopyPath({ graph, onTap, selectedUid }: CopyPathProps) {
  const { t } = useTheme();
  const stripGraph = useMemo<StripGraph>(() => {
    const mkTile = (b: EditBlock): StripBlock => {
      // `nodeTileArt` branches by node kind: a CabSim is named from its cabinet
      // (cab1; dual-cab split happens later via expandDualCab); a head-with-baked-cab /
      // half-stack amp becomes a head-over-cab tile; a COMBO amp (form-driven
      // `isComboBid`) stays a single combo tile (its built-in speaker is a cabSimId too,
      // so without the flag it would wrongly stack); else plain block art.
      return {
        ...nodeTileArt(b.model, b.cabSimId, isComboBid(b.model)),
        model: b.model,
        onClick: onTap
          ? () => {
              onTap(b.uid);
            }
          : undefined,
        selected: selectedUid != null && b.uid === selectedUid,
        change: b.change ?? undefined,
        cabSimId: b.cabSimId,
        cabSimId2: b.cabSimId2,
        cabSim2Enabled: b.cabSim2Enabled,
      };
    };
    const mkLane = <T extends string>(l: { type: T; blocks: EditBlock[] }) => ({
      type: l.type,
      blocks: l.blocks.map(mkTile),
    });
    const stages: StripStage[] = expandDualCab(
      graph.stages.map((st) =>
        st.kind === "split"
          ? { kind: "split", a: st.a.map(mkTile), b: st.b.map(mkTile) }
          : { kind: "series", blocks: st.blocks.map(mkTile) },
      ),
    );
    return {
      inputType: graph.inputType ?? undefined,
      outputType: graph.outputType ?? undefined,
      inputs: graph.inputs
        ? { a: mkLane(graph.inputs.a), b: mkLane(graph.inputs.b) }
        : null,
      outputs: graph.outputs
        ? { a: mkLane(graph.outputs.a), b: mkLane(graph.outputs.b) }
        : null,
      lanes: graph.lanes
        ? graph.lanes.map((l) => ({
            input: l.input,
            output: l.output,
            blocks: l.blocks.map(mkTile),
          }))
        : null,
      stages,
    };
  }, [graph, onTap, selectedUid]);

  return (
    <div style={{ padding: `${String(t.space5)}px 0 ${String(t.space1)}px` }}>
      <SignalChainView graph={stripGraph} size="sm" />
    </div>
  );
}

export default CopyPath;
