// src/__tests__/DoctorSetupLabels.test.tsx — BUG→GATE (issue 2), Doctor's sound names.
// Same mislabel as Level's Set up (see SetupPage.labels.test.tsx): a hardcoded "Whole
// preset" for every base row shadowed `chosenFrom`'s own answer for a multi-scene preset.
// `chosenFrom` names every main sound "Main sound" (look B's one name), and Doctor renders
// a sound's own name verbatim wherever it names one: the Check page's rows (and the
// Results rows, from the same `soundNameOf`). The Instrument page is per PRESET in look B
// (one instrument per preset), so it names presets, one row each, never sounds.

import { describe, it, expect, vi } from "vitest";
import { screen, within } from "@testing-library/react";

import { DoctorInstrument } from "../views/doctor/DoctorInstrument";
import { DoctorRun } from "../views/doctor/DoctorRun";
import { DOCTOR_STEPS } from "../views/doctor/useDoctorFlow";
import { presetsOf } from "../views/doctor/instrumentModel";
import { soundLabel, soundNameOf } from "../views/doctor/resultsModel";
import type { SetupOption } from "../views/level/leveling";
import type { DoctorInputArg } from "../lib/types";
import { renderWithTheme } from "./testKit";

const base = (over: Partial<SetupOption> = {}): SetupOption => ({
  key: "p0",
  slot: 0,
  presetName: "Friedman HBE",
  isBase: true,
  sceneSlot: null,
  sceneName: "Main sound",
  tag: "BASE",
  hasScenes: true,
  ...over,
});

const input = (o: SetupOption): DoctorInputArg => ({
  key: o.key,
  listIndex: o.slot,
  scene: o.sceneSlot,
  footswitch: null,
  label: o.key,
  soundName: o.sceneName,
  tag: o.tag,
  topologyId: null,
  calibrationLufs: null,
  profileId: null,
  nodes: [],
  footswitches: [],
  sceneOverrides: [],
});

function renderCheck(options: SetupOption[]) {
  const names = new Map(options.map((o) => [o.key, soundNameOf(o)]));
  return renderWithTheme(
    <DoctorRun
      status="running"
      items={options.map(input)}
      statusByKey={{}}
      nameOf={(k) => {
        const n = names.get(k);
        return n
          ? { preset: n.preset, sound: soundLabel(n) }
          : { preset: "", sound: k };
      }}
      steps={DOCTOR_STEPS}
      onStop={vi.fn()}
      onComplete={vi.fn()}
      onRetry={vi.fn()}
      onBack={vi.fn()}
    />,
  );
}

const rowOf = (key: string) => {
  const row = document.querySelector<HTMLElement>(`[data-check-row="${key}"]`);
  if (!row) throw new Error(`no row ${key}`);
  return within(row);
};

describe("Doctor — main sound label (issue 2)", () => {
  it('renders the sound\'s own name verbatim (`chosenFrom` names it "Main sound")', () => {
    renderCheck([base()]);
    expect(rowOf("p0").getByText("Main sound")).toBeInTheDocument();
    expect(screen.queryByText("Base Preset")).toBeNull();
    expect(screen.queryByText("Whole preset")).toBeNull();
  });

  it("never re-derives it from whether the preset has scenes", () => {
    renderCheck([
      base({
        presetName: "Studio Clean",
        sceneName: "Named by chosenFrom",
        hasScenes: false,
      }),
    ]);
    expect(rowOf("p0").getByText("Named by chosenFrom")).toBeInTheDocument();
  });
});

describe("Doctor — the Instrument page is per preset", () => {
  it("lists each chosen preset once, however many of its sounds are chosen", () => {
    const chosen = [
      base(),
      base({ key: "s0:0", isBase: false, sceneSlot: 0, sceneName: "Verse" }),
      base({ key: "s0:1", isBase: false, sceneSlot: 1, sceneName: "Chorus" }),
    ];
    renderWithTheme(
      <DoctorInstrument
        presets={presetsOf(chosen)}
        soundCount={chosen.length}
        instrumentOptions={[{ value: "none", label: "None" }]}
        recordedInstruments={new Set()}
        initial={new Map([[0, ""]])}
        onInstrumentChange={vi.fn()}
        onBack={vi.fn()}
        onRun={vi.fn()}
      />,
    );
    const rows = document.querySelectorAll("[data-instrument-row]");
    expect(rows).toHaveLength(1);
    expect(
      within(rows[0] as HTMLElement).getByText("Friedman HBE"),
    ).toBeInTheDocument();
    // No sound names on this page: it asks about the preset.
    expect(screen.queryByText("Main sound")).toBeNull();
    expect(screen.queryByText("Verse")).toBeNull();
    expect(
      screen.getByRole("button", { name: "Check 3 sounds" }),
    ).toBeDisabled();
  });

  it("points at the missing instrument with a link whose 14 px icon takes the link colour", () => {
    renderWithTheme(
      <DoctorInstrument
        presets={presetsOf([base()])}
        soundCount={1}
        instrumentOptions={[{ value: "none", label: "None" }]}
        recordedInstruments={new Set()}
        initial={new Map([[0, ""]])}
        onInstrumentChange={vi.fn()}
        onBack={vi.fn()}
        onRun={vi.fn()}
      />,
    );
    const link = screen.getByRole("button", {
      name: "Choose an instrument for Friedman HBE",
    });
    const icon = link.querySelector("svg");
    expect(icon?.getAttribute("width")).toBe("14");
    expect(icon?.getAttribute("stroke")).toBe("currentColor");
  });
});
