// src/__tests__/setupPageTestUtils.tsx — render Level's Set up page with sensible
// defaults, and drive its dropdowns by their e2e hooks (`data-select="<tid>"` on a
// trigger, `data-select-option="<tid>:<value>"` on an option). Replaces
// pickCardTestUtils.tsx: the look B `Select` needs no dialog card to portal into.

import { vi } from "vitest";
import { screen } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";

import { SetupPage } from "../views/level/SetupPage";
import type { SetupChoice, SetupOption } from "../views/level/leveling";
import type { SelectOption } from "../ui/selectValue";
import { renderWithTheme } from "./testKit";

export const INSTRUMENTS: SelectOption[] = [
  { value: "none", label: "None" },
  { value: "strat", label: "Strat" },
  { value: "tele", label: "Tele" },
];

export interface RenderSetupOptions {
  isRelevel?: boolean;
  instrumentOptions?: SelectOption[];
  recorded?: ReadonlySet<string>;
  targetNames?: string[];
  saved?: Partial<Record<number, string>>;
  defaultInst?: string;
  onCalibrate?: () => void;
  onInstrumentChange?: (slot: number, id: string) => void;
}

/** Render Set up over `options`; returns its commit and cancel mocks. */
export function renderSetupPage(
  options: SetupOption[],
  over: RenderSetupOptions = {},
) {
  const onStart = vi.fn<(c: SetupChoice[], rebalance: number[]) => void>();
  const onCancel = vi.fn();
  const view = renderWithTheme(
    <SetupPage
      options={options}
      isRelevel={over.isRelevel ?? false}
      instrumentOptions={over.instrumentOptions ?? INSTRUMENTS}
      recordedInstruments={over.recorded ?? new Set()}
      targetNames={over.targetNames ?? ["Rhythm", "Lead"]}
      savedInstruments={over.saved ?? {}}
      defaultInst={over.defaultInst ?? "none"}
      onCancel={onCancel}
      onStart={onStart}
      onInstrumentChange={over.onInstrumentChange}
      onCalibrate={over.onCalibrate}
    />,
  );
  return { onStart, onCancel, view };
}

/** A Select's trigger by its e2e hook. */
export function selectTrigger(tid: string): HTMLButtonElement {
  const el = document.querySelector<HTMLButtonElement>(
    `[data-select="${tid}"]`,
  );
  if (!el) throw new Error(`no Select with tid ${tid}`);
  return el;
}

/** Open the Select `tid` and pick its option `value`. */
export async function choose(
  user: UserEvent,
  tid: string,
  value: string,
): Promise<void> {
  await user.click(selectTrigger(tid));
  const option = document.querySelector<HTMLElement>(
    `[data-select-option="${tid}:${value}"]`,
  );
  if (!option) throw new Error(`no option ${value} in ${tid}`);
  await user.click(option);
}

/** Tick the backup acknowledgment, then click Start leveling. */
export async function startLeveling(user: UserEvent): Promise<void> {
  await user.click(
    screen.getByRole("checkbox", { name: "I've backed up with Pro Control" }),
  );
  await user.click(screen.getByRole("button", { name: "Start leveling" }));
}
