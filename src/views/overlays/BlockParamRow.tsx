// src/views/overlays/BlockParamRow.tsx — `FlatLevelPick`'s candidate row: the
// parameter's label, an optional sub-line (Recommended tag / "may change the tone" /
// "can only lower" / a disabled reason), and a trailing check icon when selected. A
// thin wrapper over the `PickListRow` chrome that supplies the param label.
// `FlatLevelPick` is this component's sole caller. (The warn-note sub-line,
// `PickWarnNote`, lives in its own module — see `PickWarnNote.tsx`.)

import { PickListRow, type PickListRowProps } from "./PickListRow";

export type BlockParamRowProps = Omit<PickListRowProps, "label"> & {
  /** The parameter's friendly label (e.g. via `paramLabel(parameterId)`). */
  paramLabel: string;
};

export function BlockParamRow({ paramLabel, ...rowProps }: BlockParamRowProps) {
  return <PickListRow {...rowProps} label={paramLabel} />;
}

export default BlockParamRow;
