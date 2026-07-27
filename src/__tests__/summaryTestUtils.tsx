import { render } from "@testing-library/react";

import { ThemeProvider } from "../theme/ThemeProvider";
import { SummaryBody } from "../views/overlays/SummaryBody";
import type { RunItem } from "../views/level/leveling";

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
