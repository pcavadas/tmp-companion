// src/views/doctor/layout.ts — the one width at which Doctor's pages change shape.
//
// From this window width up (the 1440 × 900 layout), the Instrument page keeps a 640 px
// column and the Check page a 720 px one under their titles, and Results puts the opened
// sound in a panel beside the list instead of under its row. Below it (900 × 680, and the
// 800 × 600 minimum) every page uses the full width of its column.

/** Window width (px) from which Doctor uses its wide layout. */
export const WIDE_WINDOW = 1200;
