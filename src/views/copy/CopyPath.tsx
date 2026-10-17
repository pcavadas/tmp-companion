// src/views/copy/CopyPath.tsx — the Copy feature's signal-path renderer.
//
// Copy has NO renderer of its own: it reuses the Level page's `SignalChainView` engine
// (GUITAR/MIC input nodes, OUT nodes, SPLIT/MIX/JOIN diamonds, rounded split brackets,
// procedural per-model art). This adapter maps an `EditGraph` → the engine's `StripGraph`
// and renders it. Tiles are buttons only when `onTap` is given (the source strip chooses a
// block with it, a target strip opens a block's menu); `selectedUid` draws the ring and
// `change` the +/⟲ badge. Every tile carries its block's `uid`.
//
// `gaps` (a target strip while a source block is chosen) puts a "+" (`GapPlus`) in every
// gap between blocks and at each end of a row: the engine calls `gap.render` with the
// uids either side, and a gap places the block before the block to its right, or after
// the last block of its row. With gaps on, the strip is drawn exactly as without them,
// so a dual cab keeps the two parallel cab tiles `expandDualCab` draws for it. The upper
// cab tile carries the cab's `uid`, so its "+" before and after place the block before
// or after the whole cab ("at the end" after a trailing one); the lower tile has no
// `uid`, so its lane draws no "+". The upper lane's "+" repeat the series "+" either
// side of the cab, which place the block at the same spot.

import { useMemo } from "react";

import { useTheme } from "../../theme/ThemeContext";
import { nodeTileArt } from "../../models/blockArt";
import { isComboBid } from "../../models/catalog";
import {
  SignalChainView,
  type StripBlock,
  type StripGraph,
} from "../SignalChainView";
import { toStripRoute } from "../stripExpand";
import { GapPlus } from "./GapPlus";
import { blockName, gapLabel } from "./copyText";
import { blockArrays, type EditBlock, type EditGraph } from "./copyModel";

/** The "+" gaps of a target strip. */
export interface CopyGaps {
  /** The target preset's name, for each gap's accessible name. */
  presetName: string;
  /** The chosen block would break a firmware limit anywhere in this preset. */
  disabled: boolean;
  /** Place the chosen block before or after block `uid`. */
  onPlace: (uid: string, mode: "before" | "after") => void;
}

export interface CopyPathProps {
  graph: EditGraph;
  /** Tap handler — makes the tiles buttons (the source strip, a target strip). */
  onTap?: (uid: string) => void;
  /** The chosen or open block's uid (drives the selected ring). */
  selectedUid?: string | null;
  /** A target strip while a source block is chosen: a "+" in every gap. */
  gaps?: CopyGaps;
}

/** The last block of the chain's main path, when the path ends in one series run (no
 *  split outputs or independent lanes): its trailing gap reads "at the end". */
function endUid(graph: EditGraph): string | null {
  if (graph.outputs || (graph.lanes && graph.lanes.length > 0)) return null;
  const last =
    graph.stages.length > 0 ? graph.stages[graph.stages.length - 1] : null;
  if (last?.kind !== "series" || last.blocks.length === 0) return null;
  return last.blocks[last.blocks.length - 1].uid;
}

export function CopyPath({ graph, onTap, selectedUid, gaps }: CopyPathProps) {
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
        uid: b.uid,
        bypassed: b.bypassed,
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
    const route = toStripRoute(graph, mkTile);
    if (!gaps) {
      return {
        inputType: graph.inputType ?? undefined,
        outputType: graph.outputType ?? undefined,
        ...route,
      };
    }

    const byUid = new Map<string, EditBlock>();
    for (const arr of blockArrays(graph))
      for (const b of arr) byUid.set(b.uid, b);
    const last = endUid(graph);
    const nameOf = (b: EditBlock) => blockName(b.model, b.cabSimId);
    return {
      inputType: graph.inputType ?? undefined,
      outputType: graph.outputType ?? undefined,
      ...route,
      gap: {
        render: ({ beforeUid, afterUid }) => {
          const uid = beforeUid ?? afterUid;
          const b = uid != null ? byUid.get(uid) : undefined;
          if (uid == null || !b) return null;
          const mode = beforeUid != null ? "before" : "after";
          const label = gapLabel(
            gaps.presetName,
            mode === "before"
              ? { before: nameOf(b) }
              : uid === last
                ? "end"
                : { after: nameOf(b) },
          );
          return (
            <GapPlus
              label={label}
              mode={mode}
              disabled={gaps.disabled}
              onPlace={() => {
                gaps.onPlace(uid, mode);
              }}
            />
          );
        },
      },
    };
  }, [graph, onTap, selectedUid, gaps]);

  return (
    // No top padding: the strip's own scroll box reaches 8 px up for the edit badge, into
    // the card's 10 px gap above.
    <div style={{ paddingBottom: t.sp2 }}>
      <SignalChainView graph={stripGraph} size="sm" />
    </div>
  );
}
