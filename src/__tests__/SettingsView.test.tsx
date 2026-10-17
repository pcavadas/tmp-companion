// Settings view (look B) — a SplitView of four pages: Loudness targets, Instruments,
// Playback, and About and updates.
//
//   • Targets render from the store as rows with a slider and a "−26.0 dB" value; "Add
//     target" (in the page head) appends a row in rename mode and persists via
//     save_targets (no ceiling clamp — the value seeds at −19.0, PR2 re-baseline: +3
//     from the mono-era −22.0). Deleting the last target shows the empty state.
//   • Playback: "How loud you play" persists via set_playback_level, "How your speakers
//     are connected" via set_playback_channels; one body line under each.
//   • About and updates: "TMP Companion {version}", a manual check (busy, then "Up to
//     date"), the auto-install switch, "Report a bug", and the note on where the data
//     is kept (worded for the platform) — driven off the injected `updater` prop.
//   • The sidebar is a nav of four items; no page has a footer.

import { describe, it, expect, vi, beforeEach } from "vitest";
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { SettingsView } from "../views/settings";
import { loadPlatform } from "../lib/platform";
import type { UpdaterApi } from "../lib/useUpdater";
import type { CommandName } from "../lib/commands";
import type { PlaybackChannels, PlaybackLevel, Target } from "../lib/types";
import { callsOf, makeStore, mockCommands } from "./commandMocks";
import { renderWithTheme } from "./testKit";

const SEED_TARGETS = [
  { name: "Rhythm", lufs: -26.0 },
  { name: "Crunch", lufs: -24.0 },
  { name: "Lead", lufs: -22.0 },
];

const LEVEL_BODY =
  "The louder you play, the bigger bass sounds. So when you level with a bass, sounds are set 1 dB lower at Rehearsal and 1.5 dB lower at Stage than at Quiet. Guitars don't change.";

// get_store returns the seed targets; everything else keeps the setup.ts defaults.
function mockStore(
  targets: Target[] = SEED_TARGETS,
  playback: PlaybackLevel = "stage",
  channels: PlaybackChannels = "stereo",
  os = "macos",
) {
  mockCommands({
    get_store: () =>
      makeStore({
        targets,
        playback_level: playback,
        playback_channels: channels,
      }),
    app_info: () => ({ name: "TMP Companion", version: "1.0.0", os }),
  });
}

// Named refs (not accessed through the interface-typed object) so assertions
// don't trip `@typescript-eslint/unbound-method` on the interface's method
// signatures.
const checkFn = vi.fn<() => Promise<"found" | "none" | "error">>();
const setAutoInstallFn = vi.fn<(on: boolean) => void>();

function makeStubUpdater(): UpdaterApi {
  checkFn.mockReset().mockResolvedValue("none");
  setAutoInstallFn.mockReset();
  return {
    phase: "idle",
    version: null,
    notes: null,
    percent: 0,
    autoInstall: true,
    currentVersion: "1.2.3",
    hasUpdateChannel: true,
    setAutoInstall: setAutoInstallFn,
    check: checkFn,
    review: vi.fn(),
    cancelReview: vi.fn(),
    startDownload: vi.fn(),
    dismiss: vi.fn(),
    retry: vi.fn(),
    restart: vi.fn(),
  };
}

function renderView(
  updaterOverrides: Partial<UpdaterApi> = {},
  onReportBug?: () => void,
) {
  renderWithTheme(
    <SettingsView
      connected={false}
      updater={{ ...makeStubUpdater(), ...updaterOverrides }}
      onReportBug={onReportBug}
    />,
  );
}

// The sidebar shows one page at a time (default "Loudness targets").
async function openPage(
  user: ReturnType<typeof userEvent.setup>,
  label: string,
) {
  const nav = screen.getByRole("navigation", { name: "Settings categories" });
  await user.click(within(nav).getByRole("button", { name: label }));
}

const lastArgs = <K extends CommandName>(command: K) => {
  const calls = callsOf(command);
  return calls.length ? calls[calls.length - 1] : undefined;
};

describe("SettingsView — loudness targets", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    mockStore();
  });

  it("renders the seed targets with their sliders and values in dB", async () => {
    renderView();
    expect(
      await screen.findByRole("heading", { name: "Loudness targets" }),
    ).toBeInTheDocument();
    expect(await screen.findByText("Rhythm")).toBeInTheDocument();
    expect(screen.getByText("Crunch")).toBeInTheDocument();
    expect(screen.getByText("Lead")).toBeInTheDocument();
    expect(screen.getByText("−26.0 dB")).toBeInTheDocument();
    expect(screen.getByText("−22.0 dB")).toBeInTheDocument();
    const slider = screen.getByRole("slider", { name: "Rhythm target" });
    expect(slider).toHaveAttribute("aria-valuetext", "−26.0 dB");
    expect(screen.getAllByText("Quieter")).toHaveLength(3);
    expect(screen.getAllByText("Louder")).toHaveLength(3);
    // The grip is named for what dragging it does; the ⋯ for its row.
    expect(screen.getByRole("img", { name: "Move Rhythm" })).toBeVisible();
    expect(
      screen.getByRole("button", { name: "More for Rhythm" }),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Sounds named like a target get it on Set up."),
    ).toBeInTheDocument();
    // No LUFS on screen.
    expect(screen.queryByText(/LUFS/)).toBeNull();
  });

  it("adds a target (rename mode + persists via save_targets)", async () => {
    const user = userEvent.setup();
    renderView();
    await screen.findByText("Rhythm");

    await user.click(screen.getByRole("button", { name: "Add target" }));

    // The new row opens in rename mode: an input pre-filled with "New target".
    const input = await screen.findByDisplayValue("New target");
    expect(input).toHaveAccessibleName("Target name");

    // Persisted: a 4th target seeded at −19.0 (PR2 re-baseline), no clamp.
    const saved = lastArgs("save_targets");
    expect(saved?.targets).toHaveLength(4);
    expect(saved?.targets[3]).toEqual({ name: "New target", lufs: -19.0 });
  });

  it("renames from the row's ⋯", async () => {
    const user = userEvent.setup();
    renderView();
    await screen.findByText("Rhythm");

    await user.click(screen.getByRole("button", { name: "More for Rhythm" }));
    await user.click(screen.getByRole("menuitem", { name: "Rename" }));
    const input = screen.getByRole("textbox", { name: "Target name" });
    await user.clear(input);
    await user.type(input, "Clean{Enter}");

    expect(lastArgs("save_targets")?.targets[0]).toEqual({
      name: "Clean",
      lufs: -26,
    });
  });

  it("shows the empty state once every target is deleted", async () => {
    const user = userEvent.setup();
    mockStore([{ name: "Only", lufs: -20.0 }]);
    renderView();
    await screen.findByText("Only");

    await user.click(screen.getByRole("button", { name: "More for Only" }));
    await user.click(screen.getByRole("menuitem", { name: "Delete target" }));

    expect(await screen.findByText("No targets yet")).toBeInTheDocument();
    expect(
      screen.getByText("Leveling needs at least one target."),
    ).toBeInTheDocument();
    expect(lastArgs("save_targets")?.targets).toHaveLength(0);
  });
});

describe("SettingsView — playback", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("persists how loud you play via set_playback_level, under one fixed body line", async () => {
    const user = userEvent.setup();
    mockStore();
    renderView();
    await screen.findByText("Rhythm");
    await openPage(user, "Playback");

    expect(screen.getByRole("heading", { name: "Playback" })).toBeVisible();
    const group = screen.getByRole("radiogroup", { name: "How loud you play" });
    expect(within(group).getByRole("radio", { name: "Stage" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    expect(screen.getByText(LEVEL_BODY)).toBeInTheDocument();

    await user.click(within(group).getByRole("radio", { name: "Rehearsal" }));
    await waitFor(() => {
      expect(lastArgs("set_playback_level")).toEqual({ level: "rehearsal" });
    });
    // The body doesn't change with the choice, and no LU figure is shown.
    expect(screen.getByText(LEVEL_BODY)).toBeInTheDocument();
    expect(screen.queryByText(/LU\b/)).toBeNull();
  });

  it("reflects the stored level", async () => {
    const user = userEvent.setup();
    mockStore(SEED_TARGETS, "rehearsal");
    renderView();
    await screen.findByText("Rhythm");
    await openPage(user, "Playback");

    const group = await screen.findByRole("radiogroup", {
      name: "How loud you play",
    });
    expect(
      within(group).getByRole("radio", { name: "Rehearsal" }),
    ).toHaveAttribute("aria-checked", "true");
  });

  it("defaults to Stereo and persists the speaker choice via set_playback_channels", async () => {
    const user = userEvent.setup();
    mockStore();
    renderView();
    await screen.findByText("Rhythm");
    await openPage(user, "Playback");

    const group = screen.getByRole("radiogroup", {
      name: "How your speakers are connected",
    });
    expect(
      within(group).getByRole("radio", { name: "Stereo" }),
    ).toHaveAttribute("aria-checked", "true");
    expect(
      screen.getByText(
        "Both outputs count: two speakers or stereo headphones.",
      ),
    ).toBeInTheDocument();

    await user.click(
      within(group).getByRole("radio", { name: "Mono (both outputs)" }),
    );
    await waitFor(() => {
      expect(lastArgs("set_playback_channels")).toEqual({
        channels: "mono_sum",
      });
    });
    expect(
      screen.getByText(
        "Both outputs are mixed into one: one speaker, or a mono PA fed by both outputs.",
      ),
    ).toBeInTheDocument();
    // The playback LEVEL is a separate setting: picking speakers never writes it.
    expect(callsOf("set_playback_level")).toHaveLength(0);
  });

  it("reflects the stored speaker choice", async () => {
    const user = userEvent.setup();
    mockStore(SEED_TARGETS, "stage", "left_only");
    renderView();
    await screen.findByText("Rhythm");
    await openPage(user, "Playback");

    const group = await screen.findByRole("radiogroup", {
      name: "How your speakers are connected",
    });
    expect(
      within(group).getByRole("radio", { name: "Left output only" }),
    ).toHaveAttribute("aria-checked", "true");
    expect(
      screen.getByText("Only the left output (1) is plugged in."),
    ).toBeInTheDocument();
  });
});

describe("SettingsView — about and updates", () => {
  beforeEach(async () => {
    vi.mocked(invoke).mockReset();
    mockStore();
    await act(async () => {
      await loadPlatform();
    });
  });

  it("renders the version, the check, the auto-install switch and Report a bug", async () => {
    const user = userEvent.setup();
    renderView({}, vi.fn());
    await screen.findByText("Rhythm");
    await openPage(user, "About and updates");

    expect(screen.getByText("TMP Companion 1.2.3")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Check for updates" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("switch", { name: "Install updates by themselves" }),
    ).toHaveAttribute("aria-checked", "true");
    expect(screen.getByText("Something not working?")).toBeInTheDocument();
    expect(
      screen.getByText(
        "Your instruments and loudness targets are kept on this Mac. Presets, scenes and songs live on your Tone Master Pro.",
      ),
    ).toBeInTheDocument();
  });

  it("names no version it doesn't know", async () => {
    const user = userEvent.setup();
    renderView({ currentVersion: null });
    await screen.findByText("Rhythm");
    await openPage(user, "About and updates");
    expect(screen.getByText("TMP Companion")).toBeInTheDocument();
  });

  it("checks for updates: busy while checking, then Up to date", async () => {
    const user = userEvent.setup();
    let finish: (r: "none") => void = () => undefined;
    renderView({
      check: () =>
        new Promise((res) => {
          finish = res;
        }),
    });
    await screen.findByText("Rhythm");
    await openPage(user, "About and updates");

    const button = screen.getByRole("button", { name: /Check for updates/ });
    await user.click(button);
    // The same button, busy: it keeps its words and can't be clicked again.
    expect(button).toBeDisabled();
    expect(button).toHaveTextContent("Check for updates");

    await act(async () => {
      finish("none");
      await Promise.resolve();
    });
    expect(await screen.findByText("Up to date")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Check for updates/ }),
    ).toBeNull();
  });

  // Linux ships .deb/.rpm and latest.json carries only darwin-* keys, so a check
  // there can only ever come back empty. Reporting that as "up to date" told
  // Linux users they were current forever.
  it("with no update channel, offers no check and no auto-install switch", async () => {
    const user = userEvent.setup();
    renderView({ hasUpdateChannel: false });
    await screen.findByText("Rhythm");
    await openPage(user, "About and updates");
    await screen.findByText("TMP Companion 1.2.3");

    expect(
      screen.queryByRole("button", { name: /check for updates/i }),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
    expect(screen.queryByText("Up to date")).not.toBeInTheDocument();
    expect(
      screen.getByText(
        "Linux packages update by hand. Download the latest .deb or .rpm from the releases page:",
      ),
    ).toBeInTheDocument();
    expect(screen.getByText(/releases\/latest$/)).toBeInTheDocument();
  });

  it("says 'this computer' where the data is kept off a Mac", async () => {
    const user = userEvent.setup();
    mockStore(SEED_TARGETS, "stage", "stereo", "linux");
    await act(async () => {
      await loadPlatform();
    });
    renderView({ hasUpdateChannel: false });
    await screen.findByText("Rhythm");
    await openPage(user, "About and updates");
    expect(
      screen.getByText(
        "Your instruments and loudness targets are kept on this computer. Presets, scenes and songs live on your Tone Master Pro.",
      ),
    ).toBeInTheDocument();
  });

  it("'Report a bug' (Something not working?) calls onReportBug", async () => {
    const user = userEvent.setup();
    const onReportBug = vi.fn();
    renderView({}, onReportBug);
    await screen.findByText("Rhythm");
    await openPage(user, "About and updates");
    await user.click(screen.getByRole("button", { name: "Report a bug" }));
    expect(onReportBug).toHaveBeenCalledTimes(1);
  });

  it("toggles auto-install", async () => {
    const user = userEvent.setup();
    renderView();
    await screen.findByText("Rhythm");
    await openPage(user, "About and updates");

    await user.click(
      screen.getByRole("switch", { name: "Install updates by themselves" }),
    );

    expect(setAutoInstallFn).toHaveBeenCalledWith(false);
  });
});

describe("SettingsView — sidebar", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    mockStore();
  });

  it("lists the four pages and marks the shown one", async () => {
    renderView();
    await screen.findByText("Rhythm");
    const nav = screen.getByRole("navigation", { name: "Settings categories" });
    const items = within(nav).getAllByRole("button");
    expect(items.map((el) => el.textContent)).toEqual([
      "Loudness targets",
      "Instruments",
      "Playback",
      "About and updates",
    ]);
    expect(
      within(nav).getByRole("button", { name: "Loudness targets" }),
    ).toHaveAttribute("aria-current", "true");
  });

  it("swaps the page when one is clicked; the data note lives on About only", async () => {
    const user = userEvent.setup();
    renderView();
    await screen.findByText("Rhythm");
    expect(screen.getByRole("button", { name: "Add target" })).toBeVisible();
    expect(screen.queryByText(/kept on this/)).toBeNull();

    await openPage(user, "Instruments");

    expect(screen.queryByRole("button", { name: "Add target" })).toBeNull();
    expect(
      screen.getByRole("button", { name: "Add instrument" }),
    ).toBeVisible();
    // connected={false} → one page Notice says why recording is off.
    expect(
      screen.getByText("Recording needs your Tone Master Pro connected."),
    ).toBeInTheDocument();
    expect(screen.queryByText(/kept on this/)).toBeNull();
  });
});
