// src/__tests__/AboutPane.test.tsx — the no-update-channel block is worded per
// OS: a Windows build must never show the Linux ".deb or .rpm" sentence (owner
// finding on PR #381 — Windows users saw the Linux hand-update copy, which now
// that a setup.exe ships would send them looking for packages that do not
// exist). The platform comes from the module-scoped `usePlatform` store, so the
// test mocks the hook rather than the store's loader.
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import type { ReactElement } from "react";

import { ThemeProvider } from "../theme/ThemeProvider";
import { AboutPane } from "../views/settings/AboutPane";
import type { UpdaterApi } from "../lib/useUpdater";
import { usePlatform } from "../lib/platform";

vi.mock("../lib/platform", () => ({ usePlatform: vi.fn() }));

const noChannelUpdater = (): UpdaterApi => ({
  phase: "idle",
  version: null,
  notes: null,
  percent: 0,
  autoInstall: false,
  currentVersion: "1.27.5",
  hasUpdateChannel: false,
  setAutoInstall: vi.fn(),
  check: vi.fn(),
  review: vi.fn(),
  cancelReview: vi.fn(),
  startDownload: vi.fn(),
  dismiss: vi.fn(),
  retry: vi.fn(),
  restart: vi.fn(),
});

function pane(): ReactElement {
  return (
    <ThemeProvider>
      <AboutPane updater={noChannelUpdater()} />
    </ThemeProvider>
  );
}

describe("AboutPane no-channel copy is worded per OS", () => {
  beforeEach(() => {
    vi.mocked(usePlatform).mockReset();
  });

  it("windows says setup.exe and warns about the unsigned installer", () => {
    vi.mocked(usePlatform).mockReturnValue("windows");
    render(pane());
    expect(screen.getByText(/setup\.exe/)).toBeInTheDocument();
    expect(screen.queryByText(/\.deb or \.rpm/)).not.toBeInTheDocument();
    expect(screen.getByText(/unsigned/)).toBeInTheDocument();
  });

  it("linux keeps the .deb/.rpm sentence and adds no unsigned warning", () => {
    vi.mocked(usePlatform).mockReturnValue("linux");
    render(pane());
    expect(screen.getByText(/\.deb or \.rpm/)).toBeInTheDocument();
    expect(screen.queryByText(/setup\.exe/)).not.toBeInTheDocument();
    expect(screen.queryByText(/unsigned/)).not.toBeInTheDocument();
  });
});
