// Behavior tests for the shared setup-list pieces (DS extraction from the Level
// SetupBody + Doctor DoctorSetup): SetupGroupHeader, PresetOptionRow, ApplyToBar,
// and the usePickedRows hook. Real timers (RTL fake-timer hang caveat).

import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { ThemeProvider } from "../theme/ThemeProvider";
import { SetupGroupHeader } from "../ui/SetupGroupHeader";
import { PresetOptionRow } from "../ui/PresetOptionRow";
import { ApplyToBar } from "../ui/ApplyToBar";
import { usePickedRows } from "../lib/usePickedRows";
import type { ReactNode } from "react";

function under(node: ReactNode) {
  return render(<ThemeProvider>{node}</ThemeProvider>);
}

describe("SetupGroupHeader", () => {
  it("renders the 1-based slot label + preset name", () => {
    under(<SetupGroupHeader slot={4} name="Clean Twin" />);
    expect(screen.getByText("005")).toBeTruthy(); // slotLabel(4) === "005"
    expect(screen.getByText("Clean Twin")).toBeTruthy();
  });
});

describe("PresetOptionRow", () => {
  it("renders name, tag, sub, and the trailing children", () => {
    under(
      <PresetOptionRow
        name="Rhythm"
        tag="FS1"
        sub="levels this scene against the preset’s base"
        isPicked={false}
        onTogglePick={() => undefined}
        columns="108px"
      >
        <div>trailing-pick</div>
      </PresetOptionRow>,
    );
    expect(screen.getByText("Rhythm")).toBeTruthy();
    expect(screen.getByText("FS1")).toBeTruthy();
    expect(
      screen.getByText("levels this scene against the preset’s base"),
    ).toBeTruthy();
    expect(screen.getByText("trailing-pick")).toBeTruthy();
  });

  it("omits the tag chip and sub when not provided", () => {
    under(
      <PresetOptionRow
        name="Whole preset"
        isPicked={false}
        onTogglePick={() => undefined}
        columns="200px"
      >
        <div>pick</div>
      </PresetOptionRow>,
    );
    expect(screen.getByText("Whole preset")).toBeTruthy();
    expect(screen.queryByText("FS1")).toBeNull();
  });

  it("fires onTogglePick when the checkbox cell is clicked", () => {
    const onTogglePick = vi.fn();
    under(
      <PresetOptionRow
        name="Base"
        isPicked={false}
        onTogglePick={onTogglePick}
        columns="108px"
      >
        <div />
      </PresetOptionRow>,
    );
    fireEvent.click(screen.getByRole("checkbox"));
    expect(onTogglePick).toHaveBeenCalledTimes(1);
  });

  it("reflects isPicked on the checkbox", () => {
    under(
      <PresetOptionRow
        name="Base"
        isPicked
        onTogglePick={() => undefined}
        columns="108px"
      >
        <div />
      </PresetOptionRow>,
    );
    expect(screen.getByRole("checkbox").getAttribute("aria-checked")).toBe(
      "true",
    );
  });
});

describe("ApplyToBar", () => {
  it("renders the label and hides the Clear affordance when nothing is picked", () => {
    under(
      <ApplyToBar label="Apply to all 3 sounds" onClear={() => undefined}>
        <div>pickers</div>
      </ApplyToBar>,
    );
    expect(screen.getByText("Apply to all 3 sounds")).toBeTruthy();
    expect(screen.getByText("pickers")).toBeTruthy();
    expect(screen.queryByText("Clear ticks")).toBeNull();
  });

  it("shows Clear ticks when somePicked and fires onClear", () => {
    const onClear = vi.fn();
    under(
      <ApplyToBar
        label="Instrument for the 2 ticked"
        somePicked
        onClear={onClear}
      >
        <div />
      </ApplyToBar>,
    );
    fireEvent.click(screen.getByText("Clear ticks"));
    expect(onClear).toHaveBeenCalledTimes(1);
  });
});

describe("usePickedRows", () => {
  function Harness({ keys }: { keys: string[] }) {
    const {
      picked,
      togglePick,
      clearPicked,
      somePicked,
      targetsForBulk,
      scopeLabel,
    } = usePickedRows(keys.map((key) => ({ key })));
    return (
      <div>
        <span data-testid="scope">{scopeLabel}</span>
        <span data-testid="some">{String(somePicked)}</span>
        <span data-testid="bulk">{targetsForBulk().join(",")}</span>
        <span data-testid="size">{String(picked.size)}</span>
        {keys.map((k) => (
          <button
            key={k}
            onClick={() => {
              togglePick(k);
            }}
          >
            {k}
          </button>
        ))}
        <button
          onClick={() => {
            clearPicked();
          }}
        >
          clear
        </button>
      </div>
    );
  }

  it("defaults to none-picked: all rows are bulk targets, scope = all", () => {
    under(<Harness keys={["a", "b", "c"]} />);
    expect(screen.getByTestId("some").textContent).toBe("false");
    expect(screen.getByTestId("scope").textContent).toBe("all 3 sounds");
    expect(screen.getByTestId("bulk").textContent).toBe("a,b,c");
  });

  it("toggling narrows bulk targets to the ticked rows and updates the scope", () => {
    under(<Harness keys={["a", "b", "c"]} />);
    fireEvent.click(screen.getByRole("button", { name: "a" }));
    fireEvent.click(screen.getByRole("button", { name: "c" }));
    expect(screen.getByTestId("some").textContent).toBe("true");
    expect(screen.getByTestId("size").textContent).toBe("2");
    expect(screen.getByTestId("scope").textContent).toBe("the 2 ticked");
    expect(screen.getByTestId("bulk").textContent).toBe("a,c");
  });

  it("toggling the same key twice removes it; clearPicked resets to all", () => {
    under(<Harness keys={["a", "b"]} />);
    fireEvent.click(screen.getByRole("button", { name: "a" }));
    fireEvent.click(screen.getByRole("button", { name: "a" }));
    expect(screen.getByTestId("some").textContent).toBe("false");
    expect(screen.getByTestId("bulk").textContent).toBe("a,b");

    fireEvent.click(screen.getByRole("button", { name: "b" }));
    expect(screen.getByTestId("size").textContent).toBe("1");
    fireEvent.click(screen.getByRole("button", { name: "clear" }));
    expect(screen.getByTestId("some").textContent).toBe("false");
    expect(screen.getByTestId("scope").textContent).toBe("all 2 sounds");
  });

  it("uses the singular 'sound' when exactly one row", () => {
    under(<Harness keys={["only"]} />);
    expect(screen.getByTestId("scope").textContent).toBe("all 1 sound");
  });
});
