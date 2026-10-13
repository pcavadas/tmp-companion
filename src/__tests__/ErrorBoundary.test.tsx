// Regression: the top-level ErrorBoundary must catch a render crash and show a
// themed fallback (not a blank tree). Locks Fix 3 — before it, an uncaught render
// error (e.g. a Rules-of-Hooks violation) unmounted the whole React root → blank
// window with no on-disk trace.

import { describe, it, expect, vi, beforeEach } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { ErrorBoundary } from "../ui/ErrorBoundary";
import { renderWithTheme } from "./testKit";

// log.ts imports @tauri-apps/plugin-log; stub it so componentDidCatch's logError
// path is inert under jsdom (it's also guarded by isTauri(), false in tests).
vi.mock("@tauri-apps/plugin-log", () => ({ error: () => Promise.resolve() }));

function Boom(): never {
  throw new Error("kaboom in render");
}

describe("ErrorBoundary", () => {
  beforeEach(() => {
    // React logs the caught error to console.error; silence it for a clean run.
    vi.spyOn(console, "error").mockImplementation(() => {
      /* no-op: swallow React's caught-error console output */
    });
  });

  it("renders the fallback (not blank) when a child throws", () => {
    renderWithTheme(
      <ErrorBoundary>
        <Boom />
      </ErrorBoundary>,
    );

    expect(screen.getByText("Something went wrong")).toBeInTheDocument();
    expect(screen.getByText("kaboom in render")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /reload/i })).toBeInTheDocument();
  });

  it("soft-resets via Try again: clears the error and re-mounts children", async () => {
    // Throws on the first render, succeeds after `shouldThrow` is flipped — so
    // clicking Try again (which clears the boundary's error state) re-mounts and
    // renders the healthy child without a full page reload.
    let shouldThrow = true;
    function Flaky() {
      if (shouldThrow) throw new Error("transient");
      return <div>recovered</div>;
    }

    renderWithTheme(
      <ErrorBoundary>
        <Flaky />
      </ErrorBoundary>,
    );

    expect(screen.getByText("Something went wrong")).toBeInTheDocument();
    // Both recovery affordances are present.
    expect(screen.getByRole("button", { name: /reload/i })).toBeInTheDocument();
    const tryAgain = screen.getByRole("button", { name: /try again/i });

    shouldThrow = false; // next render will succeed
    await userEvent.click(tryAgain);

    expect(await screen.findByText("recovered")).toBeInTheDocument();
    expect(screen.queryByText("Something went wrong")).not.toBeInTheDocument();
  });

  it("renders a thrown NON-Error (a bare string) as its text, not an empty box", () => {
    // A rejected backend command used to reach JS as a bare string; a render
    // that rethrows one must still show what went wrong.
    function ThrowsString(): never {
      const thrown: unknown = "the unit stopped answering";
      throw thrown;
    }
    renderWithTheme(
      <ErrorBoundary>
        <ThrowsString />
      </ErrorBoundary>,
    );

    expect(screen.getByText("Something went wrong")).toBeInTheDocument();
    expect(screen.getByText("the unit stopped answering")).toBeInTheDocument();
  });

  it("renders children unchanged when they do not throw", () => {
    renderWithTheme(
      <ErrorBoundary>
        <div>healthy child</div>
      </ErrorBoundary>,
    );

    expect(screen.getByText("healthy child")).toBeInTheDocument();
    expect(screen.queryByText("Something went wrong")).not.toBeInTheDocument();
  });
});
