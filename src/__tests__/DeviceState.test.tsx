// DeviceState — the one no-device page the four device tabs share (look B), picked by
// App's connect state. Locks each state's words, that "busy" is worded by the platform
// the BACKEND reports (`app_info().os`, never the user agent): Pro Control exists only on
// macOS and Windows, so Linux names just the other TMP Companion window — and that the
// busy page offers no button (quitting the other app is the whole fix). The Connection
// help button itself is the #304 gate in ClickOnlyHelpAndReorder.test.tsx.

import { describe, it, expect } from "vitest";
import { act, screen } from "@testing-library/react";

import { loadPlatform, platformOf } from "../lib/platform";
import { DeviceState } from "../views/DeviceState";
import { mockCommands } from "./commandMocks";
import { renderWithTheme } from "./testKit";

const PRO_CONTROL =
  "Quit Pro Control or the other TMP Companion window, and it connects here.";
const LINUX = "Quit the other TMP Companion window, and it connects here.";

async function onPlatform(os: string) {
  mockCommands({
    app_info: () => ({ name: "TMP Companion", version: "1.0.0", os }),
  });
  await act(async () => {
    await loadPlatform();
  });
}

describe("platformOf", () => {
  it("maps the backend's OS names, anything else to other", () => {
    expect(platformOf("macos")).toBe("macos");
    expect(platformOf("windows")).toBe("windows");
    expect(platformOf("linux")).toBe("linux");
    expect(platformOf("freebsd")).toBe("other");
  });
});

describe("DeviceState", () => {
  it("connecting — one line beside a spinner, no button", () => {
    renderWithTheme(<DeviceState state="connecting" />);
    expect(
      screen.getByText("Connecting to your Tone Master Pro…"),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("off — 'Plug in your Tone Master Pro' and the one Connection help button", () => {
    renderWithTheme(<DeviceState state="off" />);
    expect(
      screen.getByRole("heading", { name: "Plug in your Tone Master Pro" }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "Use a USB-C cable that carries data, and switch the Tone Master Pro on. It connects by itself.",
      ),
    ).toBeInTheDocument();
    expect(screen.getAllByRole("button").map((b) => b.textContent)).toEqual([
      "Connection help",
    ]);
  });

  it("failed — 'Couldn't connect to your Tone Master Pro' and Connection help", () => {
    renderWithTheme(<DeviceState state="failed" />);
    expect(
      screen.getByRole("heading", {
        name: "Couldn't connect to your Tone Master Pro",
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "Unplug it, plug it back in and wait a few seconds. It connects by itself.",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Connection help" }),
    ).toBeInTheDocument();
  });

  it.each([
    ["macos", PRO_CONTROL],
    ["windows", PRO_CONTROL],
    ["linux", LINUX],
  ])(
    "busy on %s — the same title, the platform's body, no button",
    async (os, body) => {
      await onPlatform(os);
      renderWithTheme(<DeviceState state="busy" />);
      expect(
        screen.getByRole("heading", {
          name: "Another app is using your Tone Master Pro",
        }),
      ).toBeInTheDocument();
      expect(screen.getByText(body)).toBeInTheDocument();
      expect(screen.queryByRole("button")).toBeNull();
    },
  );

  it("busy re-words itself when the platform lands after the page", async () => {
    await onPlatform("macos");
    renderWithTheme(<DeviceState state="busy" />);
    expect(screen.getByText(PRO_CONTROL)).toBeInTheDocument();
    await onPlatform("linux");
    expect(screen.getByText(LINUX)).toBeInTheDocument();
  });
});
