// src/views/stripExpand.ts — decompose a dual-cab CabSim block into two parallel
// cab tiles, so the signal-chain strip mirrors the unit (Pro Control draws a
// dual-cab as two cabs in parallel). Shared by the Level hero
// (`ActiveSignalChainView.toStripGraph`) and the Copy strip (`CopyPath`) so the two
// stay identical by construction.
//
// A dual-cab is ONE device node (one `ACD_CabSimTMS`, one nodeId) carrying both
// cabinets in its params (`cabSimId` + `cabSimId2`, `cabSim2Enabled`). We split it
// only at the DISPLAY layer: the underlying graph/EditGraph stays one node, and
// both rendered cab tiles inherit the source tile's `onClick`/`selected`/`change`,
// so the Copy strip still selects/edits the single CabSim container.

import { blockArtTile } from "../models/blockArt";
import type { StripBlock, StripStage } from "./SignalChainView";

/** Expand every dual-cab node inside a SERIES stage into
 *  `series(before) → split([cab1],[cab2]) → series(after)`, dropping empty
 *  before/after segments. Stages that are already a `split` (parallel templates)
 *  pass through untouched — a dual-cab there keeps its single (cab1-named) tile,
 *  the graceful fallback, since a `split` branch can't nest another split. */
export function expandDualCab(stages: StripStage[]): StripStage[] {
  // ponytail: series-only — the device never nests a dual-cab inside a split
  // branch (or inputs/outputs/lanes), so those keep the single cab1-named tile.
  // Upgrade path if that changes: recurse into split.a/split.b here.
  return stages.flatMap((st) =>
    st.kind === "series" ? expandSeries(st.blocks) : [st],
  );
}

function expandSeries(blocks: StripBlock[]): StripStage[] {
  const out: StripStage[] = [];
  let run: StripBlock[] = [];
  const flush = () => {
    if (run.length > 0) {
      out.push({ kind: "series", blocks: run });
      run = [];
    }
  };
  for (const b of blocks) {
    if (
      b.cabSim2Enabled === true &&
      b.cabSimId2 != null &&
      b.cabSimId2 !== ""
    ) {
      flush();
      // `b` is already resolved to cab1's art (toStripBlock/mkTile keyed it off
      // cabSimId); build cab2 from cabSimId2, inheriting b's interactive props so
      // tapping either tile targets the one CabSim node.
      // cabSimId2 is guaranteed present by the guard above, so this is an
      // unconditional cab-id lookup (not the conditional cabArtModel resolution).
      const cab2: StripBlock = {
        ...blockArtTile(`ACD_${b.cabSimId2}`),
        model: b.model,
        bypassed: b.bypassed,
        onClick: b.onClick,
        selected: b.selected,
        change: b.change,
      };
      out.push({ kind: "split", a: [b], b: [cab2] });
    } else {
      run.push(b);
    }
  }
  flush();
  return out;
}
