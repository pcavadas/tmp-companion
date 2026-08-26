// src/__tests__/param-class-table.test.ts — the TS half of the param-class contract.
//
// `param_class.rs`'s table loader says, in the message it panics with, that "it's the Rust
// and TS test suites that catch" a malformed `src/models/param-class.json`. That was only
// half true: nothing on this side read the file at all, so the Rust panic message was
// promising a check that did not exist. This file is that check.
//
// It is deliberately NOT a second classifier. The frontend has no local mirror of the
// classifier and must not grow one (`lib/types.ts`'s `ParamClass` doc: `class` arrives on
// the wire and the frontend never re-derives it). What this pins is the one thing the two
// languages genuinely share: the SPELLINGS. `ParamClass` is a hand-written TS union
// mirroring the Rust enum's serialized snake_case, and `CLASS_RANK` keys off it — so a
// table entry spelled with a class the union doesn't carry would classify fine in Rust,
// ship down the wire, and land in the frontend as an unranked key.

import { describe, expect, it } from "vitest";

import table from "../models/param-class.json";
import type { ParamClass } from "../lib/types";

/** Every class spelling the TS union admits, as a runtime value. Written out rather than
 *  derived (a type has no runtime form); the `satisfies` pins it to the union, so dropping
 *  a member from `types.ts` without touching this list is a compile error. */
const TS_CLASSES = [
  "level_linear",
  "level_db",
  "wet_mix",
] as const satisfies readonly ParamClass[];

/** The one spelling that is REAL in Rust but deliberately absent from the wire union:
 *  `ParamClass::Other` is "everything else, including params explicitly barred from
 *  leveling", and `level_candidates_for_node` filters those out before anything is
 *  serialized — so the frontend never sees one. It is legal in the table, illegal on the
 *  wire, and that asymmetry is the whole reason the union is shorter than the enum. */
const RUST_ONLY_CLASS = "other";

/** `range` is typed as a plain `number[]`, not a `[number, number]` tuple: the import is
 *  raw JSON, TS widens its array literals to `number[]`, and asserting the tuple would need
 *  a double cast through `unknown`. Nothing here reads the endpoints — the range assertion
 *  is a presence check — so the loose element type costs nothing. */
interface Entry {
  class: string;
  range?: number[];
}

/** Every `{class, range}` entry in the table, with a path for the failure message:
 *  `defaults.*`, `ampOverrides.*`, and the two-level `blockOverrides.<block>.<param>`. */
function allEntries(): [string, Entry][] {
  const t = table as {
    defaults: Record<string, Entry>;
    ampOverrides: Record<string, Entry>;
    blockOverrides: Record<string, Record<string, Entry>>;
  };
  const out: [string, Entry][] = [];
  for (const [k, v] of Object.entries(t.defaults))
    out.push([`defaults.${k}`, v]);
  for (const [k, v] of Object.entries(t.ampOverrides))
    out.push([`ampOverrides.${k}`, v]);
  for (const [block, params] of Object.entries(t.blockOverrides)) {
    for (const [k, v] of Object.entries(params))
      out.push([`blockOverrides.${block}.${k}`, v]);
  }
  return out;
}

describe("param-class.json", () => {
  it("parses, with all three sections populated", () => {
    // A guard against the check itself rotting: a table reshape that emptied one of the
    // three sections would otherwise make every assertion below pass over nothing. Named
    // per section rather than as one total, so the failure says WHICH one vanished.
    const t = table as {
      defaults: Record<string, unknown>;
      ampOverrides: Record<string, unknown>;
      blockOverrides: Record<string, Record<string, unknown>>;
    };
    expect(Object.keys(t.defaults).length).toBeGreaterThan(5);
    expect(Object.keys(t.ampOverrides).length).toBeGreaterThan(0);
    expect(Object.keys(t.blockOverrides).length).toBeGreaterThan(0);
    expect(allEntries().length).toBeGreaterThan(10);
  });

  it("spells every class either as a TS ParamClass or as the wire-absent 'other'", () => {
    const allowed = new Set<string>([...TS_CLASSES, RUST_ONLY_CLASS]);
    const bad = allEntries()
      .filter(([, v]) => !allowed.has(v.class))
      .map(([path, v]) => `${path} = ${JSON.stringify(v.class)}`);
    expect(
      bad,
      `unknown class spelling(s) — they would classify in Rust, ride the wire, and land ` +
        `in the frontend as a key CLASS_RANK/lib/types.ts do not carry`,
    ).toEqual([]);
  });

  it("exercises every TS ParamClass spelling at least once", () => {
    // The converse direction: a union member no table entry uses is either dead or a typo,
    // and either way the frontend is ranking a class the backend can never send.
    const used = new Set(allEntries().map(([, v]) => v.class));
    for (const c of TS_CLASSES) {
      expect(used.has(c), `no param-class.json entry is classed ${c}`).toBe(
        true,
      );
    }
  });

  it("gives every level_db entry an explicit range", () => {
    // Mirrors `param_class::table()`'s own assertion. A dB param has no universal range
    // (`ACD_Boost.gain` is [0,12], `makeupgaindb` [0,24]), so a missing one degenerates to
    // (0,0) — every seed and bracket point at 0.0, a silently dead solve. Checked here too
    // so the failure surfaces in `bun run test`, not only on the first Rust `classify()`.
    const bad = allEntries()
      .filter(([, v]) => v.class === "level_db" && !v.range)
      .map(([path]) => path);
    expect(bad, "level_db entries without a range").toEqual([]);
  });
});
