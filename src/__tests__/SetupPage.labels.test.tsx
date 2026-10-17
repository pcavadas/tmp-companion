// src/__tests__/SetupPage.labels.test.tsx — BUG→GATE (issue 2): Set up once hardcoded
// "Whole preset" for EVERY main sound, silently relabeling a multi-scene preset's row
// that `chosenFrom` had named differently. The fix: `chosenFrom` is the one place that
// names a row, and Set up renders `o.sceneName` verbatim, never re-deriving it.
//
// Look B gives the main sound ONE name everywhere, "Main sound" (never "Base Preset" or
// "Whole preset"), with or without scenes — so the gate now pins that
// one name at its source and the verbatim render.

import { describe, it, expect } from "vitest";
import { screen } from "@testing-library/react";

import {
  MAIN_SOUND,
  baseKey,
  chosenFrom,
  type PresetRow,
} from "../views/level/leveling";
import type { FootswitchInfo, SceneInfo } from "../lib/types";
import { makeSetupOption } from "./commandMocks";
import { renderSetupPage } from "./setupPageTestUtils";

const rows: PresetRow[] = [
  { slot: 0, name: "Friedman HBE", empty: false },
  { slot: 1, name: "Studio Clean", empty: false },
];
const sceneInfo = new Map<number, SceneInfo[]>([
  [0, [{ name: "Rhythm", fs: 1 }]],
]);
const noSwitches = new Map<number, FootswitchInfo[]>();

describe("Set up — the main sound's name (issue 2)", () => {
  it('chosenFrom names the main sound "Main sound", with or without scenes', () => {
    const out = chosenFrom(
      new Set([baseKey(0), baseKey(1)]),
      rows,
      sceneInfo,
      noSwitches,
    );
    expect(out.map((o) => [o.presetName, o.sceneName])).toEqual([
      ["Friedman HBE", MAIN_SOUND],
      ["Studio Clean", MAIN_SOUND],
    ]);
    expect(MAIN_SOUND).toBe("Main sound");
  });

  it("Set up renders the row's own name verbatim", () => {
    renderSetupPage([makeSetupOption({ presetName: "Friedman HBE" })]);
    expect(screen.getByText("Main sound")).toBeInTheDocument();
    expect(screen.queryByText("Base Preset")).toBeNull();
    expect(screen.queryByText("Whole preset")).toBeNull();
  });

  it("never re-derives it from whether the preset has scenes", () => {
    renderSetupPage([
      makeSetupOption({
        presetName: "Studio Clean",
        sceneName: "Named by chosenFrom",
        hasScenes: false,
        tag: null,
      }),
    ]);
    expect(screen.getByText("Named by chosenFrom")).toBeInTheDocument();
  });
});
