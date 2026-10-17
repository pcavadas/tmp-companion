// src/__tests__/summaryTestUtils.tsx — shared fixtures for the Summary suites
// (`summaryModel.test.ts`, `SummaryPage.test.tsx`): a leveled-row factory per sound kind
// and a themed `SummaryPage` render helper.

import { vi } from "vitest";

import { SummaryPage } from "../views/level/SummaryPage";
import type { RunItem } from "../views/level/leveling";
import { renderWithTheme } from "./testKit";

/** A leveled main sound of "Guitar" (list index 3), target Lead. */
export const base = (over: Partial<RunItem>): RunItem => ({
  key: "p3",
  slot: 3,
  presetName: "Guitar",
  isBase: true,
  sceneSlot: null,
  sceneName: "Main sound",
  footswitch: null,
  instId: "none",
  targetName: "Lead",
  status: "result",
  outcome: "done",
  value: -19,
  ...over,
});

/** A leveled scene `sceneSlot` of "Guitar", target Lead. */
export const sceneRow = (sceneSlot: number, over: Partial<RunItem>): RunItem =>
  base({
    key: `s3:${String(sceneSlot)}`,
    isBase: false,
    sceneSlot,
    sceneName: `Scene ${String(sceneSlot + 1)}`,
    ...over,
  });

/** A leveled block-acting footswitch (switch 1, "Boost") of "Guitar", in the main sound. */
export const fsRow = (over: Partial<RunItem>): RunItem =>
  base({
    key: "f3:0",
    isBase: false,
    sceneSlot: null,
    sceneName: "Boost",
    footswitch: {
      switchIndex: 0,
      levGroupId: "G1",
      levNodeId: "amp",
      levParameterId: "outputLevel",
      sceneContext: null,
    },
    ...over,
  });

/** Mount `SummaryPage` over `items` (not stopped by default); returns its callbacks. */
export function renderSummary(
  items: RunItem[],
  over: {
    stopped?: boolean;
    reampStillOn?: boolean;
    sceneOverrides?: Partial<Record<string, number | null>>;
  } = {},
) {
  const onTryAgain = vi.fn<(keys: readonly string[]) => void>();
  const onDone = vi.fn();
  const onSceneChange =
    vi.fn<(key: string, scene: number | null | undefined) => void>();
  const view = renderWithTheme(
    <SummaryPage
      items={items}
      stopped={over.stopped ?? false}
      reampStillOn={over.reampStillOn ?? false}
      targetLufsByName={(name) => (name === "Lead" ? -19 : -23)}
      sceneOverrides={over.sceneOverrides ?? {}}
      onSceneChange={onSceneChange}
      onTryAgain={onTryAgain}
      onDone={onDone}
    />,
  );
  return { onTryAgain, onDone, onSceneChange, view };
}
