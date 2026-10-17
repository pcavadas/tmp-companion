// src/__tests__/RunPieces.test.tsx — useAutoAdvance, the run pages' natural-finish →
// summary jump (Level's run → Summary, Doctor's Check → Results).

import { afterEach, beforeEach, describe, it, expect, vi } from "vitest";
import { act } from "@testing-library/react";

import { useAutoAdvance } from "../lib/useAutoAdvance";
import { renderWithTheme } from "./testKit";

function Advancer(props: {
  done: boolean;
  stopped: boolean;
  onAdvance: () => void;
  delayMs?: number;
}) {
  useAutoAdvance(props.done, props.stopped, props.onAdvance, props.delayMs);
  return null;
}

// Fake timers: the advance is a plain timeout, and nothing here awaits a `findBy`/`waitFor`
// (which would poll on the frozen clock).
describe("useAutoAdvance", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  /** Run the clock 60 ms — three times the 20 ms delay under test. */
  const elapse = () => {
    act(() => {
      vi.advanceTimersByTime(60);
    });
  };

  it("fires after a natural finish (done && !stopped)", () => {
    const onAdvance = vi.fn();
    renderWithTheme(
      <Advancer done stopped={false} onAdvance={onAdvance} delayMs={20} />,
    );
    act(() => {
      vi.advanceTimersByTime(19);
    });
    expect(onAdvance).not.toHaveBeenCalled();
    elapse();
    expect(onAdvance).toHaveBeenCalledTimes(1);
  });

  it("does NOT fire after a manual stop (done && stopped)", () => {
    const onAdvance = vi.fn();
    renderWithTheme(
      <Advancer done stopped onAdvance={onAdvance} delayMs={20} />,
    );
    elapse();
    expect(onAdvance).not.toHaveBeenCalled();
  });

  it("does NOT fire while the run is still going (not done)", () => {
    const onAdvance = vi.fn();
    renderWithTheme(
      <Advancer
        done={false}
        stopped={false}
        onAdvance={onAdvance}
        delayMs={20}
      />,
    );
    elapse();
    expect(onAdvance).not.toHaveBeenCalled();
  });
});
