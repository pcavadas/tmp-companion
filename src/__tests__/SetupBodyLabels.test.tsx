// src/__tests__/SetupBodyLabels.test.tsx — BUG→GATE (issue 2): SetupPage's Base row
// hardcoded "Whole preset" for EVERY preset, silently relabeling a multi-scene
// preset's base row (which `chosenFrom` already names "Base Preset"). The fix is
// `nameLabel = o.sceneName` — `chosenFrom` is the one place that decides the string,
// keyed off `hasScenes`; SetupPage must render it verbatim, never re-derive it.

import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";

import { ThemeProvider } from "../theme/ThemeProvider";
import { SetupPage } from "../views/level/SetupPage";
import type { SetupOption } from "../views/level/leveling";
import type { PickOption } from "../views/overlays/Pick";

const instrumentOptions: PickOption[] = [{ id: "none", label: "None" }];
const targetOptions: PickOption[] = [{ id: "Rhythm", label: "Rhythm −18" }];

function renderSetup(option: SetupOption) {
  return render(
    <ThemeProvider>
      <SetupPage
        options={[option]}
        isRelevel={false}
        instrumentOptions={instrumentOptions}
        targetOptions={targetOptions}
        defaultInst="none"
        defaultTarget="Rhythm"
        onCancel={vi.fn()}
        onStart={vi.fn()}
      />
    </ThemeProvider>,
  );
}

describe("SetupPage — base row label (issue 2)", () => {
  it('a hasScenes base row renders "Base Preset"', () => {
    renderSetup({
      key: "p0",
      slot: 0,
      presetName: "Friedman HBE",
      isBase: true,
      sceneSlot: null,
      sceneName: "Base Preset",
      tag: "BASE",
      hasScenes: true,
    });
    expect(screen.getByText("Base Preset")).toBeInTheDocument();
    expect(screen.queryByText("Whole preset")).toBeNull();
  });

  it('a scene-less base row renders "Whole preset"', () => {
    renderSetup({
      key: "p0",
      slot: 0,
      presetName: "Studio Clean",
      isBase: true,
      sceneSlot: null,
      sceneName: "Whole preset",
      tag: null,
      hasScenes: false,
    });
    expect(screen.getByText("Whole preset")).toBeInTheDocument();
    expect(screen.queryByText("Base Preset")).toBeNull();
  });
});
