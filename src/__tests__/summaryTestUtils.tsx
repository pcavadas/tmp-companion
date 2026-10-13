// src/__tests__/summaryTestUtils.tsx — shared fixtures for the five Summary*.test.tsx
// suites: a saved-Base-row factory and a themed `SummaryPage` render helper.

import { SummaryPage } from "../views/level/SummaryPage";
import type { RunItem } from "../views/level/leveling";
import { renderWithTheme } from "./testKit";

/** Shared saved-Base-row factory for the Summary* suites. */
export const base = (over: Partial<RunItem>): RunItem => ({
  key: "p3",
  slot: 3,
  presetName: "Guitar",
  isBase: true,
  sceneSlot: null,
  sceneName: "Whole preset",
  tag: null,
  footswitch: null,
  instId: "none",
  targetName: "Lead",
  status: "result",
  outcome: "done",
  value: -22,
  ...over,
});

/** Mount `SummaryPage` over `items`, not stopped, with no-op callbacks. */
export const renderSummary = (items: RunItem[]) =>
  renderWithTheme(
    <SummaryPage
      items={items}
      stopped={false}
      onAccept={() => undefined}
      onRelevel={() => undefined}
    />,
  );
