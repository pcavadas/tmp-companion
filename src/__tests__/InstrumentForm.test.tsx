// InstrumentForm (look B) — the add / edit instrument form: Name, Type (Guitar / Bass /
// Bass VI), and a Pickup Select of that type's pickups.
//
// The Pickup dropdown always shows the pickup that will be saved (danger.md's Pick
// trap). A new instrument starts on its type's first pickup and a type change moves to
// the new type's first pickup, both shown. A saved pickup the backend no longer offers
// is never swapped for options[0]: the dropdown is empty in the attention look, a note
// says why, and Save waits for a choice.

import { describe, it, expect, vi } from "vitest";
import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { InstrumentForm } from "../views/settings/InstrumentForm";
import type { Profile, TopologyInfo } from "../lib/types";
import { renderWithTheme } from "./testKit";

// The backend's shape (topologies.rs): ids are keys, `instrument` groups them.
const TOPOLOGIES: TopologyInfo[] = [
  { id: "guitar-singlecoil", label: "Single-coil", instrument: "guitar" },
  { id: "guitar-humbucker", label: "Humbucker", instrument: "guitar" },
  { id: "guitar-acoustic", label: "Acoustic / piezo", instrument: "guitar" },
  { id: "bass-singlecoil", label: "Bass single-coil", instrument: "bass" },
  { id: "bass-humbucker", label: "Bass humbucker", instrument: "bass" },
  { id: "bass-vi", label: "Bass VI", instrument: "bass-vi" },
];
const TYPES = ["guitar", "bass", "bass-vi"];

function renderForm(initial?: Profile) {
  const onSave = vi.fn();
  renderWithTheme(
    <InstrumentForm
      initial={initial}
      types={TYPES}
      topologies={TOPOLOGIES}
      onSave={onSave}
      onCancel={vi.fn()}
    />,
  );
  return { onSave, user: userEvent.setup() };
}

const pickup = () => screen.getByRole("button", { name: /^Pickup:/ });

describe("InstrumentForm", () => {
  it("offers the three instrument types in words — Acoustic is a pickup, not a type", () => {
    renderForm();
    const types = screen.getByRole("radiogroup", { name: "Type" });
    expect(
      within(types)
        .getAllByRole("radio")
        .map((el) => el.textContent),
    ).toEqual(["Guitar", "Bass", "Bass VI"]);
    expect(within(types).queryByRole("radio", { name: /Acoustic/ })).toBeNull();
  });

  it("adds an instrument on the type's first pickup, shown as chosen", async () => {
    const { onSave, user } = renderForm();
    expect(pickup()).toHaveAccessibleName("Pickup: Single-coil");
    const add = screen.getByRole("button", { name: "Add instrument" });
    expect(add).toBeDisabled(); // no name yet

    await user.type(screen.getByLabelText("Name"), "Telecaster");
    await user.click(add);
    expect(onSave).toHaveBeenCalledWith({
      name: "Telecaster",
      topology_id: "guitar-singlecoil",
    });
  });

  it("a type change moves to that type's first pickup, and a choice is saved", async () => {
    const { onSave, user } = renderForm();
    await user.type(screen.getByLabelText("Name"), "P-Bass");
    await user.click(screen.getByRole("radio", { name: "Bass" }));
    expect(pickup()).toHaveAccessibleName("Pickup: Bass single-coil");

    await user.click(pickup());
    const list = screen.getByRole("listbox");
    expect(
      within(list)
        .getAllByRole("option")
        .map((el) => el.textContent),
    ).toEqual(["Bass single-coil", "Bass humbucker"]);
    await user.click(
      within(list).getByRole("option", { name: "Bass humbucker" }),
    );
    expect(pickup()).toHaveAccessibleName("Pickup: Bass humbucker");

    await user.click(screen.getByRole("button", { name: "Add instrument" }));
    expect(onSave).toHaveBeenCalledWith({
      name: "P-Bass",
      topology_id: "bass-humbucker",
    });
  });

  it("an edit opens on the saved type and pickup", () => {
    renderForm({
      id: "p1",
      name: "Jaguar VI",
      topology_id: "bass-vi",
      calibration_lufs: null,
    });
    expect(screen.getByRole("radio", { name: "Bass VI" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    expect(pickup()).toHaveAccessibleName("Pickup: Bass VI");
    expect(
      screen.getByRole("button", { name: "Save instrument" }),
    ).toBeEnabled();
  });

  it("a saved pickup no longer offered shows the attention state — never options[0] — and Save waits", async () => {
    const { onSave, user } = renderForm({
      id: "p1",
      name: "Strat",
      topology_id: "guitar-retired-pickup",
      calibration_lufs: -20,
    });
    // Nothing is shown as chosen: not "Single-coil" (options[0]), not the raw id.
    expect(pickup()).toHaveAccessibleName("Pickup: not chosen");
    expect(pickup()).toHaveAttribute("data-attention");
    expect(pickup()).not.toHaveTextContent("Single-coil");
    expect(
      screen.getByText("The saved pickup isn’t offered any more. Choose one."),
    ).toBeInTheDocument();
    const save = screen.getByRole("button", { name: "Save instrument" });
    expect(save).toBeDisabled();

    await user.click(pickup());
    await user.click(screen.getByRole("option", { name: "Humbucker" }));
    expect(pickup()).toHaveAccessibleName("Pickup: Humbucker");
    expect(pickup()).not.toHaveAttribute("data-attention");
    expect(
      screen.queryByText(
        "The saved pickup isn’t offered any more. Choose one.",
      ),
    ).toBeNull();
    await user.click(save);
    expect(onSave).toHaveBeenCalledWith({
      name: "Strat",
      topology_id: "guitar-humbucker",
    });
  });
});
