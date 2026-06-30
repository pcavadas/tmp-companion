// src/lib/gates.ts — the acknowledgement-gate storage keys, in one place.
//
// The startup Disclaimer (App.tsx → Disclaimer.tsx) persists its "don't show again"
// state in web storage — session-scoped accept in sessionStorage, with an opt-in
// permanent accept in localStorage. Keep every read/write keyed through these constants
// so the gate can't drift across call sites and tests.
//
// (The leveling wizard's "Back up" step is ALWAYS step 1 — gated per-run by an
// acknowledgment checkbox, never persisted — so there is no Level-warning dismiss key.)

/** localStorage — disclaimer permanently acknowledged ("Don't show again"). */
export const DISCLAIMER_PERM_KEY = "tmp_disclaimer_perm";
/** sessionStorage — disclaimer acknowledged for this session. */
export const DISCLAIMER_SESSION_KEY = "tmp_disclaimer";
