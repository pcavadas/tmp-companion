// src/lib/usePickedRows.ts — bulk-pick bookkeeping shared by the two setup steps.
//
// The Leveling SetupBody and the Doctor DoctorSetup both carry a byte-identical
// "which rows are ticked for the apply-to brush" selection: a Set of row keys, a
// toggle, a clear, the somePicked flag, the bulk-target key list (ticked rows, or
// ALL rows when none are ticked), and the scope label. Lifted here verbatim.

import { useState } from "react";

export interface PickedRows {
  /** The ticked row keys. */
  picked: Set<string>;
  togglePick: (key: string) => void;
  clearPicked: () => void;
  /** true when at least one row is ticked. */
  somePicked: boolean;
  /** Keys the apply-to brush writes to: ticked rows, or ALL rows when none ticked. */
  targetsForBulk: () => string[];
  /** "the N ticked" / "all N sounds" — the apply-to bar scope phrase. */
  scopeLabel: string;
}

export function usePickedRows(options: { key: string }[]): PickedRows {
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const togglePick = (key: string) => {
    setPicked((p) => {
      const n = new Set(p);
      if (n.has(key)) n.delete(key);
      else n.add(key);
      return n;
    });
  };
  const clearPicked = () => {
    setPicked(new Set());
  };
  const somePicked = picked.size > 0;
  const targetsForBulk = (): string[] =>
    (somePicked ? options.filter((o) => picked.has(o.key)) : options).map(
      (o) => o.key,
    );
  const total = options.length;
  const scopeLabel = somePicked
    ? `the ${String(picked.size)} ticked`
    : `all ${String(total)} sound${total === 1 ? "" : "s"}`;
  return {
    picked,
    togglePick,
    clearPicked,
    somePicked,
    targetsForBulk,
    scopeLabel,
  };
}
