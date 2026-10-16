// Regression: the top-level ErrorBoundary must catch a render crash and show the
// "Something went wrong" page (not a blank tree), and log what was thrown. Locks Fix 3 —
// before it, an uncaught render error (e.g. a Rules-of-Hooks violation) unmounted the
// whole React root → blank window with no on-disk trace. Look B draws no raw message:
// the page says the details are saved for a bug report, and they are (the log).

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
    expect(
      screen.getByText("The details are saved for a bug report."),
    ).toBeInTheDocument();
    expect(screen.queryByText("kaboom in render")).not.toBeInTheDocument();
    expect(console.error).toHaveBeenCalledWith(
      expect.stringContaining("React render crash: kaboom in render"),
    );
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

  it("logs a thrown NON-Error (a bare string) as its text, and never draws it", () => {
    // A rejected backend command used to reach JS as a bare string; a render
    // that rethrows one must still record what went wrong.
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
    expect(
      screen.queryByText("the unit stopped answering"),
    ).not.toBeInTheDocument();
    expect(console.error).toHaveBeenCalledWith(
      expect.stringContaining("React render crash: the unit stopped answering"),
    );
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
