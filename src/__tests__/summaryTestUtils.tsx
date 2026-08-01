import { render } from "@testing-library/react";

import { ThemeProvider } from "../theme/ThemeProvider";
import { SummaryBody } from "../views/overlays/SummaryBody";
import type { RunItem } from "../views/level/leveling";

/** Shared saved-Base-row factory for the Summary* suites (one copy, two consumers). */
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

/** Shared SummaryBody mount for the Summary* suites (one copy, three consumers). */
export const renderSummary = (items: RunItem[]) =>
  render(
    <ThemeProvider>
      <SummaryBody
        items={items}
        stopped={false}
        onAccept={() => undefined}
        onRelevel={() => undefined}
      />
    </ThemeProvider>,
  );
