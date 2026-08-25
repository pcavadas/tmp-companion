// src/views/overlays/BlockParamRow.tsx — the CONTROL dropdown's row (the second
// stage of `BlockLevelPick`'s two-dropdown picker, D2/Part C): the parameter's
// label, an optional sub-line (Recommended tag / "may change the tone" / "can
// only lower" / a disabled reason), and a trailing check icon when selected. No
// block art here any more — the block is already fixed by the FIRST dropdown
// (`BlockPickRow`, which carries the art tile); this row only ever lists ONE
// block's own params. A thin wrapper over the row chrome shared with
// `BlockPickRow` (`PickListRow`) — this file only supplies the "param" label
// treatment and its own e2e hook attribute. `BlockLevelPick` is this component's
// sole caller. (The shared warn-note sub-line, `PickWarnNote`, moved to its own
// module once it grew callers outside this file — see `PickWarnNote.tsx`.)

import { PickListRow, type PickListRowProps } from "./PickListRow";

export type BlockParamRowProps = Omit<
  PickListRowProps,
  "leading" | "labelEmphasis" | "dataAttr" | "label"
> & {
  /** The parameter's friendly label (e.g. via `paramLabel(parameterId)`). */
  paramLabel: string;
};

export function BlockParamRow({ paramLabel, ...rowProps }: BlockParamRowProps) {
  return (
    <PickListRow
      {...rowProps}
      label={paramLabel}
      labelEmphasis="param"
      dataAttr="data-block-param-pick"
    />
  );
}

export default BlockParamRow;
