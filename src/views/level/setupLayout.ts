// src/views/level/setupLayout.ts — the Set up page's column grid, shared by its column
// heads and every sound row so the two can't drift: Sound, Target, Block, Knob, and
// "Level it in scene" only when a chosen footswitch needs it.

/** CSS grid columns for the Set up rows. */
export function setupColumns(sceneColumn: boolean): string {
  return sceneColumn
    ? "minmax(0, 1fr) 100px 110px 120px 120px"
    : "minmax(0, 1fr) 100px 110px 120px";
}

/** The gap between the columns (px). */
export const SETUP_COLUMN_GAP = 10;

/** Side padding of every Set up row and preset header (px). The column heads sit one
 *  px further in, past the list's 1 px border, so each head lines up with its column. */
export const SETUP_ROW_PAD_X = 14;

/** The flow's steps, shown at the right of each Level flow page's title. */
export const LEVEL_STEPS = ["Set up", "Level", "Summary"] as const;
