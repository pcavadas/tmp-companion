// src/__tests__/InlineEdit.test.tsx — the ONE inline text editor (#314) behind the
// setlist-title rename, the target-name rename and the new-setlist field: Enter
// commits, blur commits (unless opted out), Escape cancels, the commit is the
// TRIMMED draft, and a commit/cancel is never followed by a second one.

import { describe, it, expect, vi } from "vitest";
import { screen, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { InlineEdit } from "../ui/InlineEdit";
import { renderWithTheme } from "./testKit";

function setup(props: Partial<Parameters<typeof InlineEdit>[0]> = {}) {
  const onCommit = vi.fn<(v: string) => void>();
  const onCancel = vi.fn<() => void>();
  renderWithTheme(
    <InlineEdit
      initial="Old"
      ariaLabel="Name"
      onCommit={onCommit}
      onCancel={onCancel}
      {...props}
    />,
  );
  const input = screen.getByRole("textbox", { name: "Name" });
  return { onCommit, onCancel, input };
}

describe("InlineEdit", () => {
  it("mounts focused with the initial text", () => {
    const { input } = setup();
    expect(input).toHaveValue("Old");
    expect(input).toHaveFocus();
  });

  it("Enter commits the trimmed draft, once", async () => {
    const { onCommit, onCancel, input } = setup();
    await userEvent.clear(input);
    await userEvent.type(input, "  New name  {Enter}");
    fireEvent.blur(input); // the unmount blur that can follow an Enter
    expect(onCommit).toHaveBeenCalledTimes(1);
    expect(onCommit).toHaveBeenCalledWith("New name");
    expect(onCancel).not.toHaveBeenCalled();
  });

  it("an IME's Enter (composing) neither commits nor ends the edit", async () => {
    const { onCommit, onCancel, input } = setup();
    fireEvent.keyDown(input, { key: "Enter", isComposing: true });
    fireEvent.keyDown(input, { key: "Escape", isComposing: true });
    expect(onCommit).not.toHaveBeenCalled();
    expect(onCancel).not.toHaveBeenCalled();
    await userEvent.type(input, "{Enter}");
    expect(onCommit).toHaveBeenCalledWith("Old");
  });

  it("blur commits", async () => {
    const { onCommit, input } = setup();
    await userEvent.type(input, "er");
    fireEvent.blur(input);
    expect(onCommit).toHaveBeenCalledWith("Older");
  });

  it("Escape cancels, and the blur after it does not commit", async () => {
    const { onCommit, onCancel, input } = setup();
    await userEvent.type(input, "X{Escape}");
    fireEvent.blur(input);
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onCommit).not.toHaveBeenCalled();
  });

  it("hands an empty draft to the caller (validation is the caller's)", async () => {
    const { onCommit, input } = setup();
    await userEvent.clear(input);
    await userEvent.type(input, "   {Enter}");
    expect(onCommit).toHaveBeenCalledWith("");
  });

  it("commitOnBlur={false}: blur does nothing; the ✓ / ✕ buttons commit / cancel", async () => {
    const { onCommit, onCancel, input } = setup({
      initial: "",
      commitOnBlur: false,
      commitLabel: "Create",
    });
    await userEvent.type(input, "Gig");
    fireEvent.blur(input);
    expect(onCommit).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Create" }));
    expect(onCommit).toHaveBeenCalledWith("Gig");
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCancel).not.toHaveBeenCalled(); // the edit already ended
  });

  it("the ✕ button cancels", async () => {
    const { onCommit, onCancel } = setup({
      commitOnBlur: false,
      commitLabel: "Create",
    });
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onCommit).not.toHaveBeenCalled();
  });

  it("selectOnFocus selects the whole draft", () => {
    const { input } = setup({ selectOnFocus: true });
    const el = input as HTMLInputElement;
    fireEvent.focus(el);
    expect(el.selectionStart).toBe(0);
    expect(el.selectionEnd).toBe(3);
  });
});
