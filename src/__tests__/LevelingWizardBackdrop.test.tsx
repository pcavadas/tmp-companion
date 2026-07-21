// src/__tests__/LevelingWizardBackdrop.test.tsx — the Run and Summary stages must
// never be dismissed by a stray backdrop click: Run because it's an in-progress
// device write, Summary because it can carry actionable follow-ups (Re-level
// clamped…) that a stray click would otherwise silently discard.

import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import type { ReactElement } from "react";

import { ThemeProvider } from "../theme/ThemeProvider";
import { LevelingWizard } from "../views/overlays/LevelingWizard";
import type { LevelingWizardProps } from "../views/overlays/LevelingWizard";

function wizard(overrides: Partial<LevelingWizardProps>): ReactElement {
  const props: LevelingWizardProps = {
    stage: "summary",
    chosen: [],
    flowPresetCount: 0,
    isRelevel: false,
    instrumentOptions: [],
    targetOptions: [],
    defaultInst: "none",
    defaultTarget: "",
    instrumentName: () => "",
    runItems: [],
    runCurrentIndex: 0,
    runTotal: 0,
    runDone: true,
    runStopped: false,
    runStopping: false,
    liveLufs: null,
    liveTrace: [],
    onCancel: vi.fn(),
    onStart: vi.fn(),
    onRunCancel: vi.fn(),
    onRunComplete: vi.fn(),
    onAccept: vi.fn(),
    onRelevel: vi.fn(),
    ...overrides,
  };
  return (
    <ThemeProvider>
      <LevelingWizard {...props} />
    </ThemeProvider>
  );
}

function clickBackdrop() {
  const dialog = screen.getByRole("dialog");
  const backdrop = dialog.previousElementSibling;
  if (!backdrop) throw new Error("backdrop element not found");
  fireEvent.click(backdrop);
}

describe("LevelingWizard — Run and Summary backdrop is inert", () => {
  it("summary: clicking the backdrop does not close the wizard", () => {
    const onAccept = vi.fn();
    const onCancel = vi.fn();
    render(wizard({ stage: "summary", onAccept, onCancel }));
    clickBackdrop();
    expect(onAccept).not.toHaveBeenCalled();
    expect(onCancel).not.toHaveBeenCalled();
  });

  it("run: clicking the backdrop does not stop or close the run", () => {
    const onRunCancel = vi.fn();
    const onCancel = vi.fn();
    render(
      wizard({
        stage: "run",
        runDone: false,
        onRunCancel,
        onCancel,
      }),
    );
    clickBackdrop();
    expect(onRunCancel).not.toHaveBeenCalled();
    expect(onCancel).not.toHaveBeenCalled();
  });
});
