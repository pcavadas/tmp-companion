// src/ui/selectValue.ts — what a `Select` shows for a value: the one rule, shared by the
// component and by any caller that needs to say why (a row's note under a stale value).
//
// The danger.md Pick/BlockPick trap, closed here for every dropdown: a value the current
// options don't contain must NEVER display `options[0]` (a store without a "Crunch"
// target once displayed "Rhythm" while the run got "crunch" — a silent wrong-target
// write). A dropdown shows exactly the value that will be used, or says nothing is
// chosen. Split from Select.tsx so that file exports only its component (React Fast
// Refresh's component-boundary rule).

/** One choice in a `Select`'s list. */
export interface SelectOption {
  /** The value that will be used. */
  value: string;
  /** What the dropdown shows for it. */
  label: string;
  /** A short grey note after the label in the open list ("Volume only, best"). */
  note?: string;
  /** Shown but not choosable (the note says why). */
  disabled?: boolean;
}

/** How a value resolves against the options:
 *  - `empty`: nothing is chosen (`""`) — the dropdown shows its placeholder.
 *  - `option`: the value is offered — the dropdown shows that option's label.
 *  - `stale`: the value is set but no longer offered — the dropdown shows the stored
 *    value itself (never another option), and the caller's note says why. */
export type SelectResolution =
  | { kind: "empty" }
  | { kind: "option"; option: SelectOption }
  | { kind: "stale" };

export function resolveSelectValue(
  value: string,
  options: readonly SelectOption[],
): SelectResolution {
  if (value === "") return { kind: "empty" };
  const option = options.find((o) => o.value === value);
  return option ? { kind: "option", option } : { kind: "stale" };
}
