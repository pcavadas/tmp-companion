// src/views/overlays/BlockPickRow.tsx — the BLOCK dropdown's row (the first stage
// of `BlockLevelPick`'s two-dropdown picker, D2/Part C, split off `BlockParamRow`
// when that combined block+param list grew too long for one flat dropdown): a
// BlockArt tile, the block's full name, an optional sub-line (a "Recommended" tag
// or the shared disabled reason when every one of the block's candidates is
// disabled), and a trailing check icon when selected. A thin wrapper over the row
// chrome shared with `BlockParamRow` (`PickListRow`) — this file only supplies the
// leading art tile, the "block" label treatment, and its own e2e hook attribute.
// `BlockLevelPick` is this component's sole caller.

import { BlockArt } from "../../ui/BlockArt";
import { PickListRow, type PickListRowProps } from "./PickListRow";
import type { BlockArtFields } from "../../models/blockArt";

export interface BlockPickRowProps extends Omit<
  PickListRowProps,
  "leading" | "labelEmphasis" | "dataAttr"
> {
  /** Resolved via `blockArtTile(fenderId)`. */
  art: BlockArtFields;
}

const TILE = 38;
const ART_SIZE = 34;

export function BlockPickRow({ art, ...rowProps }: BlockPickRowProps) {
  return (
    <PickListRow
      {...rowProps}
      labelEmphasis="block"
      dataAttr="data-block-pick"
      leading={
        <span
          style={{
            width: TILE,
            height: TILE,
            flexShrink: 0,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
          }}
        >
          <BlockArt
            icon={art.icon}
            tone={art.tone}
            footswitch={art.footswitch}
            bodyColor={art.body}
            panelColor={art.panel}
            accentColor={art.accent}
            label={false}
            size={ART_SIZE}
          />
        </span>
      }
    />
  );
}

export default BlockPickRow;
