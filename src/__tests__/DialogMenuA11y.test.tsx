// src/__tests__/DialogMenuA11y.test.tsx — non-regression gate for #308 (the DS Dialog and
// Menu shells' modality, naming, focus and clipping bugs).
//
// Dialog: portals to document.body with an accessible name; the app root (`#root`) is
// `inert` while any dialog is open and only loses it when the LAST stacked dialog closes;
// a child's `autoFocus` keeps focus (the card no longer steals it).
// Menu: portals out of an `overflow: auto` scroller (not clipped) and flips above a
// trigger at the window bottom; `role` comes from the caller; one outside click both
// closes the menu AND reaches the clicked control (no click-eating Scrim).
// Toast: portals to document.body too, so an open dialog's inert root can't make its ×
// fall through to the dialog's backdrop.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";

import { ThemeProvider } from "../theme/ThemeProvider";
import { Dialog } from "../ui/Dialog";
import { Menu } from "../ui/Menu";
import { Toast } from "../ui/primitives";

let appRoot: HTMLDivElement;

beforeEach(() => {
  appRoot = document.createElement("div");
  appRoot.id = "root";
  document.body.appendChild(appRoot);
});

afterEach(() => {
  appRoot.remove();
  vi.restoreAllMocks();
});

function renderInRoot(ui: React.ReactNode) {
  return render(<ThemeProvider>{ui}</ThemeProvider>, { container: appRoot });
}

describe("Dialog — modal for AT and Tab, named, focus-safe (#308)", () => {
  it("portals into document.body (outside the app root) with an accessible name", () => {
    renderInRoot(
      <Dialog label="Test sheet">
        <div>content</div>
      </Dialog>,
    );
    const dialog = screen.getByRole("dialog", { name: "Test sheet" });
    expect(appRoot.contains(dialog)).toBe(false);
    expect(document.body.contains(dialog)).toBe(true);
  });

  it("makes the app root inert while open and clears it on close", () => {
    const { rerender } = renderInRoot(
      <Dialog label="One">
        <div>content</div>
      </Dialog>,
    );
    expect(appRoot.hasAttribute("inert")).toBe(true);
    rerender(<ThemeProvider>{null}</ThemeProvider>);
    expect(appRoot.hasAttribute("inert")).toBe(false);
  });

  it("keeps the root inert until the LAST of two stacked dialogs closes", () => {
    function Stack({ outer, inner }: { outer: boolean; inner: boolean }) {
      return (
        <>
          {outer && (
            <Dialog label="Outer">
              <div>outer</div>
            </Dialog>
          )}
          {inner && (
            <Dialog label="Inner" zIndex={60}>
              <div>inner</div>
            </Dialog>
          )}
        </>
      );
    }
    const { rerender } = renderInRoot(<Stack outer inner />);
    expect(appRoot.hasAttribute("inert")).toBe(true);
    // Close the inner (confirm) first — the outer sheet is still open.
    rerender(
      <ThemeProvider>
        <Stack outer inner={false} />
      </ThemeProvider>,
    );
    expect(appRoot.hasAttribute("inert")).toBe(true);
    rerender(
      <ThemeProvider>
        <Stack outer={false} inner={false} />
      </ThemeProvider>,
    );
    expect(appRoot.hasAttribute("inert")).toBe(false);

    // And the other order: outer closes first, inner still holds the root inert.
    rerender(
      <ThemeProvider>
        <Stack outer inner />
      </ThemeProvider>,
    );
    rerender(
      <ThemeProvider>
        <Stack outer={false} inner />
      </ThemeProvider>,
    );
    expect(appRoot.hasAttribute("inert")).toBe(true);
    rerender(
      <ThemeProvider>
        <Stack outer={false} inner={false} />
      </ThemeProvider>,
    );
    expect(appRoot.hasAttribute("inert")).toBe(false);
  });

  it("a child autoFocus input keeps focus (the card does not steal it)", () => {
    renderInRoot(
      <Dialog label="Rename">
        <input aria-label="Name" autoFocus />
      </Dialog>,
    );
    expect(document.activeElement).toBe(screen.getByLabelText("Name"));
  });

  it("with no focusable child, the card itself takes focus", () => {
    renderInRoot(
      <Dialog label="Plain">
        <div>content</div>
      </Dialog>,
    );
    expect(document.activeElement).toBe(screen.getByRole("dialog"));
  });

  it("restores focus to the opener on close", async () => {
    const user = userEvent.setup();
    function Harness() {
      const [open, setOpen] = useState(false);
      return (
        <>
          <button
            onClick={() => {
              setOpen(true);
            }}
          >
            Open
          </button>
          {open && (
            <Dialog
              label="Sheet"
              onClose={() => {
                setOpen(false);
              }}
            >
              <div>content</div>
            </Dialog>
          )}
        </>
      );
    }
    renderInRoot(<Harness />);
    const opener = screen.getByRole("button", { name: "Open" });
    await user.click(opener);
    expect(appRoot.hasAttribute("inert")).toBe(true);
    const backdrop = screen.getByRole("dialog").previousElementSibling;
    if (!(backdrop instanceof HTMLElement)) throw new Error("no backdrop");
    await user.click(backdrop);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.activeElement).toBe(opener);
  });
});

describe("Menu — portaled, flipping, caller-owned role, non-eating dismiss (#308)", () => {
  function MenuHarness({
    role,
    onOther,
  }: {
    role?: "menu" | "dialog";
    onOther?: () => void;
  }) {
    const [open, setOpen] = useState(true);
    return (
      <>
        <div data-testid="scroller" style={{ overflowY: "auto", height: 40 }}>
          <span data-testid="host" style={{ position: "relative" }}>
            <button
              onClick={() => {
                setOpen((o) => !o);
              }}
            >
              Trigger
            </button>
            {open && (
              <Menu
                role={role}
                label={role === "dialog" ? "Add things" : undefined}
                onClose={() => {
                  setOpen(false);
                }}
              >
                <div>Item</div>
              </Menu>
            )}
          </span>
        </div>
        <button onClick={onOther}>Other control</button>
      </>
    );
  }

  it("renders the card into document.body, outside the overflow scroller", () => {
    renderInRoot(<MenuHarness />);
    const card = screen.getByRole("menu");
    expect(screen.getByTestId("scroller").contains(card)).toBe(false);
    expect(card.parentElement).toBe(document.body);
    expect(card.style.position).toBe("fixed");
  });

  it("flips ABOVE a trigger at the bottom of the window instead of clipping", () => {
    vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(100);
    vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(140);
    const bottom = window.innerHeight - 10;
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue(
      DOMRect.fromRect({ x: 300, y: bottom - 20, width: 20, height: 20 }),
    );
    renderInRoot(<MenuHarness />);
    const card = screen.getByRole("menu");
    // Above the trigger's top (bottom - 20) by the card height + gap.
    expect(card.style.top).toBe(`${String(bottom - 20 - 100 - 4)}px`);
    // Right edges flush with the anchor box.
    expect(card.style.left).toBe(`${String(320 - 140)}px`);
  });

  it("takes its role from the caller (a form popover is a named dialog)", () => {
    renderInRoot(<MenuHarness role="dialog" />);
    expect(screen.getByRole("dialog", { name: "Add things" })).toBeTruthy();
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("one outside click closes the menu AND reaches the clicked control", async () => {
    const onOther = vi.fn();
    const user = userEvent.setup();
    renderInRoot(<MenuHarness onOther={onOther} />);
    expect(screen.getByRole("menu")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Other control" }));
    expect(screen.queryByRole("menu")).toBeNull();
    expect(onOther).toHaveBeenCalledTimes(1);
  });

  it("a click inside the card does not dismiss it", async () => {
    const user = userEvent.setup();
    renderInRoot(<MenuHarness />);
    await user.click(screen.getByText("Item"));
    expect(screen.getByRole("menu")).toBeTruthy();
  });

  it("the trigger still toggles the menu closed (its press is not 'outside')", async () => {
    const user = userEvent.setup();
    renderInRoot(<MenuHarness />);
    await user.click(screen.getByRole("button", { name: "Trigger" }));
    expect(screen.queryByRole("menu")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Trigger" }));
    expect(screen.getByRole("menu")).toBeTruthy();
  });
});

describe("Toast — stays clickable over an open dialog (#308)", () => {
  it("portals outside the inert app root, and its dismiss fires without closing the dialog", async () => {
    const onDismiss = vi.fn();
    const onClose = vi.fn();
    renderInRoot(
      <>
        <Dialog label="Sheet" onClose={onClose}>
          <div>content</div>
        </Dialog>
        <Toast title="Saved" onDismiss={onDismiss} />
      </>,
    );
    expect(appRoot.hasAttribute("inert")).toBe(true);
    const dismiss = screen.getByRole("button", { name: "Dismiss" });
    expect(appRoot.contains(dismiss)).toBe(false);
    await userEvent.click(dismiss);
    expect(onDismiss).toHaveBeenCalledTimes(1);
    expect(onClose).not.toHaveBeenCalled();
  });
});
