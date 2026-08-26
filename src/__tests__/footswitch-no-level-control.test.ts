// src/__tests__/footswitch-no-level-control.test.ts — BUG→GATE: a footswitch with
// NO level-class parameter (e.g. "Friedman HBE" FS2 "PHASER" → `ACD_PhaserP90`, which
// carries zero level_params) must never be silently dropped, and `childKeys` (the
// selection-count source) must agree EXACTLY with the row builder (`chosenFrom`) on
// which footswitch keys are selectable — both must resolve off the ONE shared
// predicate `footswitchLevelable`, never a duplicated `level_params.length` check.
//
// Before the fix: `childKeys` counted every footswitch (including one with zero
// level_params) as selectable, while `chosenFrom` silently dropped it from the built
// SetupOption[] — a selection the tree said was "1 of 1" vanished with no explanation
// once the wizard's Set-up list was built from it.

import { describe, it, expect } from "vitest";

import {
  childKeys,
  chosenFrom,
  footswitchLevelable,
  fswKey,
} from "../views/level/leveling";
import type { PresetRow } from "../views/PresetList";
import type { FootswitchInfo } from "../lib/types";

const LEVELABLE: FootswitchInfo = {
  switch: 0,
  label: "Drive",
  link_group: null,
  functions: [],
  level_params: [
    {
      group_id: "G1",
      node_id: "N0",
      fender_id: "ACD_BluesDriver",
      parameter_id: "gain",
      current: 0.5,
      class: "level_linear",
    },
  ],
};

// Mirrors the real incident: FS2 "PHASER" → ACD_PhaserP90, zero level_params.
const NO_LEVEL_CONTROL: FootswitchInfo = {
  switch: 1,
  label: "PHASER",
  link_group: null,
  functions: [
    {
      func: "on-off",
      group_id: "G2",
      node_id: "N1",
      fender_id: "ACD_PhaserP90",
      parameter_id: null,
      value_a: null,
      value_b: null,
      is_active: false,
    },
  ],
  level_params: [],
};

describe("footswitchLevelable", () => {
  it("is true iff the switch carries at least one level candidate", () => {
    expect(footswitchLevelable(LEVELABLE)).toBe(true);
    expect(footswitchLevelable(NO_LEVEL_CONTROL)).toBe(false);
  });
});

describe("childKeys — footswitch selectability (BUG 1)", () => {
  it("excludes a footswitch with no level-class parameter from the selectable set", () => {
    const keys = childKeys(0, [], [LEVELABLE, NO_LEVEL_CONTROL]);
    // Only the Base key + the ONE levelable footswitch's key (by its ORIGINAL array
    // position, index 0) — the unlevelable one at position 1 must not appear at all.
    expect(keys).toEqual(["p0", fswKey(0, 0)]);
    expect(keys).not.toContain(fswKey(0, 1));
  });

  it("still addresses a levelable footswitch by its ORIGINAL position when an earlier sibling is unlevelable", () => {
    // NO_LEVEL_CONTROL first, LEVELABLE second — the survivor's key must stay keyed
    // to its true array position (index 1), not get renumbered to 0 by a naive
    // filter-then-map (which would silently repoint every later switch's key).
    const keys = childKeys(0, [], [NO_LEVEL_CONTROL, LEVELABLE]);
    expect(keys).toEqual(["p0", fswKey(0, 1)]);
  });
});

describe("chosenFrom — the row builder must agree with childKeys (BUG 1)", () => {
  const rows: PresetRow[] = [{ slot: 0, name: "Friedman HBE", empty: false }];
  const footswitchInfo = new Map([[0, [LEVELABLE, NO_LEVEL_CONTROL]]]);

  it("builds a SetupOption for the levelable footswitch when selected", () => {
    const sel = new Set([fswKey(0, 0)]);
    const options = chosenFrom(sel, rows, new Map(), footswitchInfo);
    expect(options).toHaveLength(1);
    expect(options[0]?.footswitch?.switchIndex).toBe(0);
  });

  it("never emits a row for the no-level-control footswitch, even if its key is (erroneously) selected", () => {
    // Defends the "must not be counted/selectable ANYWHERE" invariant even against a
    // hypothetical stray selection — the row builder's own null-target guard must
    // independently refuse it, not merely rely on childKeys never producing the key.
    const sel = new Set([fswKey(0, 1)]);
    const options = chosenFrom(sel, rows, new Map(), footswitchInfo);
    expect(options).toHaveLength(0);
  });

  it("a whole-preset selection (childKeys-derived) never pulls in the unlevelable footswitch", () => {
    const sel = new Set(childKeys(0, [], [LEVELABLE, NO_LEVEL_CONTROL]));
    const options = chosenFrom(sel, rows, new Map(), footswitchInfo);
    // Base + the one levelable footswitch only.
    expect(options).toHaveLength(2);
    expect(options.some((o) => o.footswitch?.switchIndex === 1)).toBe(false);
  });
});
