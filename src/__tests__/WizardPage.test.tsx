// src/__tests__/WizardPage.test.tsx — the shared full-page wizard frame (#314) the
// Level wizard and the Doctor's Set up / Results pages render on: the rail shows the
// caller's `steps` with `current` marked, the Back link appears only with `back` and
// calls it, and the page provides itself as the Pick-family portal target.

import { describe, it, expect, vi } from "vitest";
import { useContext, useEffect } from "react";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { WizardPage } from "../views/overlays/WizardPage";
import { DialogCardCtx, WIZ_STEPS } from "../views/overlays/wizardContext";
import { DOCTOR_STEPS } from "../views/doctor/useDoctorFlow";
import { renderWithTheme } from "./testKit";

interface PortalProbeProps {
  seen: (el: HTMLDivElement | null) => void;
}

/** Reports the portal target it sees, after commit (refs are read in an effect). */
function PortalProbe({ seen }: PortalProbeProps) {
  const ref = useContext(DialogCardCtx);
  useEffect(() => {
    seen(ref?.current ?? null);
  });
  return null;
}

describe("WizardPage", () => {
  it("renders the caller's rail and a Back link that calls `back`", async () => {
    const back = vi.fn<() => void>();
    renderWithTheme(
      <WizardPage steps={DOCTOR_STEPS} current={0} back={back}>
        <p>body</p>
      </WizardPage>,
    );
    for (const s of DOCTOR_STEPS) {
      expect(screen.getByText(s.label)).toBeInTheDocument();
    }
    expect(screen.getByText("body")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Back" }));
    expect(back).toHaveBeenCalledTimes(1);
  });

  it("defaults to the leveling rail and has no Back without `back`", () => {
    renderWithTheme(
      <WizardPage current={1}>
        <p>body</p>
      </WizardPage>,
    );
    for (const s of WIZ_STEPS) {
      expect(screen.getByText(s.label)).toBeInTheDocument();
    }
    expect(screen.queryByRole("button", { name: "Back" })).toBeNull();
  });

  it("marks the steps before `current` done (a check, not a number)", () => {
    renderWithTheme(
      <WizardPage current={1}>
        <p>body</p>
      </WizardPage>,
    );
    // Step 1 is done → its number is replaced by a check; steps 2 and 3 show numbers.
    expect(screen.queryByText("1")).toBeNull();
    expect(screen.getByText("2")).toBeInTheDocument();
    expect(screen.getByText("3")).toBeInTheDocument();
  });

  it("provides the page as the DialogCardCtx portal target", () => {
    const seen = vi.fn<(el: HTMLDivElement | null) => void>();
    renderWithTheme(
      <WizardPage current={0}>
        <p>body</p>
        <PortalProbe seen={seen} />
      </WizardPage>,
    );
    const el = seen.mock.lastCall?.[0] ?? null;
    expect(el).not.toBeNull();
    // The target is the page itself: it holds the rail and the body.
    expect(el?.textContent).toContain("Set up");
    expect(el?.textContent).toContain("body");
  });
});
