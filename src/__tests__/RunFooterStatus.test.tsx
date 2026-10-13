// src/__tests__/RunFooterStatus.test.tsx — the run footer's shared primary slot
// (#314), used by the Doctor check and the Level run: a landed stop shows the ONE
// CTA (with the page's own label), a natural finish shows a static "done" marker
// (never a button — auto-advance is already moving on), otherwise the caller's
// running content. Stopped wins over done.

import { describe, it, expect, vi } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { RunFooterStatus } from "../ui/RunFooterStatus";
import { renderWithTheme } from "./testKit";

function setup(stopped: boolean, done: boolean, ctaLabel = "Continue") {
  const onCta = vi.fn<() => void>();
  renderWithTheme(
    <RunFooterStatus
      stopped={stopped}
      done={done}
      ctaLabel={ctaLabel}
      onCta={onCta}
      running={<span>still going</span>}
    />,
  );
  return onCta;
}

describe("RunFooterStatus", () => {
  it("running: renders the caller's content, no CTA, no marker", () => {
    setup(false, false);
    expect(screen.getByText("still going")).toBeInTheDocument();
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.queryByText("done")).toBeNull();
  });

  it("done: a static marker, not a button", () => {
    setup(false, true);
    expect(screen.getByText("done")).toBeInTheDocument();
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.queryByText("still going")).toBeNull();
  });

  it("stopped: the one CTA with the page's label, which calls onCta", async () => {
    const onCta = setup(true, false, "See results");
    await userEvent.click(screen.getByRole("button", { name: "See results" }));
    expect(onCta).toHaveBeenCalledTimes(1);
  });

  it("stopped wins over done", () => {
    setup(true, true);
    expect(
      screen.getByRole("button", { name: "Continue" }),
    ).toBeInTheDocument();
    expect(screen.queryByText("done")).toBeNull();
  });
});
