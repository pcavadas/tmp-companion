// src/views/doctor/matchModel.ts — pure "Match reference" EQ-move math: given
// two sounds' mean-removed per-band balanceDb (`DoctorSoundResult.balanceDb`,
// level already cancels by construction), derive the graphic-EQ-10 moves
// that bring one toward the other. No device I/O — the UI (MatchCard.tsx)
// wraps the result in a synthetic `DoctorRx` and reuses PrescriptionCard's
// existing apply/save/discard flow.

import type { DoctorOp, GraphNode } from "../../lib/types";

/** Geometric band centers (Hz) — sqrt(lo*hi) per band, VERIFIED against
 *  `src-tauri/src/doctor.rs`'s `BANDS_6`/`BANDS_7` (via `Family::bands()` +
 *  `Family::band_centers()`). Keyed by the display label
 *  (`DoctorSoundResult.bandLabels`), not index, so it's layout-order-proof;
 *  keep in lockstep with the Rust table if it ever changes. */
export const BAND_CENTER_HZ: Partial<Record<string, number>> = {
  // Sub (bass-vi only, ~42 Hz) is deliberately EXCLUDED: its own log-nearest
  // EQ-10 band is `gain31hz` (verified — NOT gain62hz, despite Lows' nearby
  // ~85 Hz center landing there), and gain31hz is HW-unverified/no-op-prone
  // (doctor.rs ~:1686-1691's EQ10_BANDS comment). So Sub is skipped entirely
  // (eqMovesFor already no-ops on a band with no known center) rather than
  // mapped to a band we can't confirm actually moves — a deliberate
  // divergence from the Rust EQ10_BANDS mirror.
  Lows: Math.sqrt(60 * 120),
  "Low-mids": Math.sqrt(120 * 400),
  Mids: Math.sqrt(400 * 1000),
  "High-mids": Math.sqrt(1000 * 3000),
  Highs: Math.sqrt(3000 * 6000),
  Air: Math.sqrt(6000 * 12000),
};

/** The 10 EQ-10 graphic-EQ bands (Hz, controlId), ascending — mirrors
 *  `doctor.rs`'s `EQ10_BANDS`. */
const EQ10_BANDS: [number, string][] = [
  [31, "gain31hz"],
  [62, "gain62hz"],
  [125, "gain125hz"],
  [250, "gain250hz"],
  [500, "gain500hz"],
  [1000, "gain1khz"],
  [2000, "gain2khz"],
  [4000, "gain4khz"],
  [8000, "gain8khz"],
  [16000, "gain16khz"],
];

const MAX_GAIN_DB = 6;
const DROP_BELOW_DB = 1.5;

/** The stock stereo EQ-10 block — never the Mono variant (absent from the
 *  product profile). Mirrors `doctor.rs`'s `EQ10_STEREO`. */
export const EQ10_STEREO = "ACD_TenBandEQStereo";

/** EQ-10 band gain range (dB) — mirrors `doctor.rs`'s `EQ10_BAND_RANGE_DB`
 *  (±12 is the graphic-EQ standard; the band controlIds' fw schema is the
 *  source to re-derive from if a rev differs). */
export const EQ10_BAND_RANGE_DB = 12;

/** An existing, drivable EQ-10 already in the chain, or null when none — the
 *  reuse-vs-insert gate for the Match-reference fix (mirrors `doctor.rs`'s
 *  `eq_move`/`graph_facts`, which reuse `facts.eq10` instead of stacking a
 *  second EQ-10). A bypassed EQ-10 doesn't count (`graph_facts`'s loop
 *  `continue`s past a bypassed node before it ever reaches the EQ-10 check).
 *  When SEVERAL non-bypassed EQ-10s exist, `graph_facts` has no `is_none()`
 *  guard on `f.eq10` (unlike `f.cab`/`f.reverb_mix`) — it overwrites on every
 *  match, so the LAST one in node order wins; mirrored here for parity. */
export function existingEq10(nodes: GraphNode[]): GraphNode | null {
  let found: GraphNode | null = null;
  for (const n of nodes) {
    if (!n.bypassed && n.model === EQ10_STEREO) found = n;
  }
  return found;
}

/** Reuse-branch ops for an existing EQ-10: per move, `current + gainDb`
 *  clamped to +/-`EQ10_BAND_RANGE_DB`, missing current value treated as 0 —
 *  mirrors `doctor.rs`'s `eq_move` reuse branch (the `known` case; a fresh
 *  reuse always writes relative-to-current, so there's no separate "unknown
 *  current value" branch to mirror here). */
export function eq10ReuseOps(eq: GraphNode, moves: EqMove[]): DoctorOp[] {
  return moves.map((m) => {
    const current = eq.params[m.controlId] ?? 0;
    const value = Math.max(
      -EQ10_BAND_RANGE_DB,
      Math.min(EQ10_BAND_RANGE_DB, current + m.gainDb),
    );
    return {
      kind: "param",
      groupId: eq.group_id,
      nodeId: eq.node_id,
      param: m.controlId,
      value,
    };
  });
}

/** True when two sounds share an IDENTICAL band layout (same labels, same
 *  order) with coherent balance arrays — the precondition for every
 *  index-paired computation below. Equal COUNT is not enough: Bass and
 *  Bass VI are both 7 bands with different meanings per index. The one gate
 *  both the Match-reference offer (`SoundRow.canMatch`) and the pairing
 *  itself (`MatchCard`) share, so they can never disagree. */
export function bandLayoutsMatch(
  ref: { bandLabels: string[]; balanceDb: number[] },
  sound: { bandLabels: string[]; balanceDb: number[] },
): boolean {
  return (
    ref.bandLabels.length === sound.bandLabels.length &&
    ref.balanceDb.length === ref.bandLabels.length &&
    sound.balanceDb.length === sound.bandLabels.length &&
    ref.bandLabels.every((label, i) => label === sound.bandLabels[i])
  );
}

/** Per-band delta (ref − sound) toward the reference. Both inputs are
 *  mean-removed balanceDb, so absolute level cancels by construction — this
 *  is a pure spectral-SHAPE comparison. */
export function matchDeltas(ref: number[], sound: number[]): number[] {
  const n = Math.min(ref.length, sound.length);
  const out: number[] = [];
  for (let i = 0; i < n; i++) {
    out.push(ref[i] - sound[i]);
  }
  return out;
}

/** Log-frequency-nearest EQ-10 band to `hz` — octave-spaced bands need log
 *  distance, not linear (mirrors `doctor.rs`'s `nearest_band`). */
function nearestEq10Band(hz: number): string {
  let bestId = EQ10_BANDS[0][1];
  let bestDist = Infinity;
  for (const [bandHz, id] of EQ10_BANDS) {
    const dist = Math.abs(Math.log(bandHz) - Math.log(hz));
    if (dist < bestDist) {
      bestDist = dist;
      bestId = id;
    }
  }
  return bestId;
}

export interface EqMove {
  controlId: string;
  gainDb: number;
}

/** Map per-family-band deltas to EQ-10 graphic-EQ moves: each band's delta
 *  lands on the log-nearest of the 10 EQ-10 bands; when several family bands
 *  land on the same EQ band their deltas are AVERAGED; each combined move is
 *  clamped to +/-6 dB and rounded to the nearest 0.5 dB, then moves under
 *  1.5 dB (below audibility here) are dropped. Returned ascending by EQ band
 *  frequency. A band with no known center (`BAND_CENTER_HZ`) is skipped —
 *  defensive, shouldn't occur on real `bandLabels`. */
export function eqMovesFor(deltas: number[], bandLabels: string[]): EqMove[] {
  const sums = new Map<string, number>();
  const counts = new Map<string, number>();
  for (let i = 0; i < deltas.length && i < bandLabels.length; i++) {
    const center = BAND_CENTER_HZ[bandLabels[i]];
    if (center == null) continue;
    const controlId = nearestEq10Band(center);
    sums.set(controlId, (sums.get(controlId) ?? 0) + deltas[i]);
    counts.set(controlId, (counts.get(controlId) ?? 0) + 1);
  }
  const order = new Map(EQ10_BANDS.map(([, id], i) => [id, i]));
  const moves: EqMove[] = [];
  for (const [controlId, sum] of sums) {
    const avg = sum / (counts.get(controlId) ?? 1);
    const clamped = Math.max(-MAX_GAIN_DB, Math.min(MAX_GAIN_DB, avg));
    const gainDb = Math.round(clamped * 2) / 2;
    if (Math.abs(gainDb) < DROP_BELOW_DB) continue;
    moves.push({ controlId, gainDb });
  }
  moves.sort(
    (a, b) => (order.get(a.controlId) ?? 0) - (order.get(b.controlId) ?? 0),
  );
  return moves;
}

/** True when any single family-band delta exceeds what a clamped +/-6 dB EQ
 *  move can close — a spectral gap too big for an EQ alone to bridge. */
export function matchResidualLarge(deltas: number[]): boolean {
  return deltas.some((d) => Math.abs(d) > MAX_GAIN_DB);
}

/** Player-facing frequency label for an EQ-10 controlId: `gain250hz` → "250
 *  Hz", `gain2khz` → "2 kHz" (mirrors `doctor.rs`'s `eq_band_label`). */
export function eqBandLabel(controlId: string): string {
  const core = controlId.replace(/^gain/, "").replace(/hz$/, "");
  return core.endsWith("k") ? `${core.slice(0, -1)} kHz` : `${core} Hz`;
}

/** The group id to APPEND the Match-reference EQ-10 to: the LAST guitar-chain
 *  node's group. Guitar groups are identified the same way the backend does
 *  (`doctor.rs`'s `graph_facts`'s `front` field): `group_id` starting with
 *  "G" (mic groups start "M"). This is by construction the last "G"-prefixed
 *  index in the whole node list, so nothing in its own group follows it —
 *  the insert is always an append (`beforeFenderId: null` at the call site).
 *  Returns `null` when the chain has no guitar nodes at all — the caller
 *  doesn't offer Apply then. */
export function lastGuitarGroup(nodes: GraphNode[]): string | null {
  let lastIdx = -1;
  for (let i = 0; i < nodes.length; i++) {
    if (nodes[i].group_id.startsWith("G")) lastIdx = i;
  }
  return lastIdx === -1 ? null : nodes[lastIdx].group_id;
}
