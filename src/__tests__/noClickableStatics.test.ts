// src/__tests__/noClickableStatics.test.ts — non-regression gate for #307: every
// clickable control is a real <button> (keyboard- and VoiceOver-reachable, Space/Enter
// activation for free), never a div/span/li/label with an onClick.
//
// It scans every src/**/*.tsx source file for a LOWERCASE (intrinsic) JSX opening tag
// that carries `onClick=` — multi-line tags included — and fails unless the tag is a
// natively interactive element or one of the allowlisted backdrop catchers below.
// Components (`<Button …>`, `<MenuItem …>`) are not intrinsic and are not scanned: they
// render real buttons themselves. Test files are skipped (a test may render a bare
// <button> fixture; it ships nothing).

import { readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";
import { describe, expect, it } from "vitest";

const SRC = join(process.cwd(), "src");

/** Elements whose onClick is fine: they are natively focusable and operable. */
const INTERACTIVE = new Set(["button", "a", "input", "select", "textarea"]);

/** Full-surface click-catchers behind an overlay: an outside click dismisses it. They
 *  are deliberately NOT buttons — each is an invisible mouse affordance over the page,
 *  and its overlay keeps its own named close/choice controls. Keyed by file, the value
 *  is the MOST onClick-bearing statics that file may hold. */
const BACKDROP_ALLOWLIST: Record<string, number> = {
  // The ONE DS dialog's blurred scrim (backdrop click closes the dialog).
  "ui/Dialog.tsx": 1,
  // The Pick dropdown's fallback full-viewport catcher (outside click closes it).
  "views/overlays/PickInlineMenu.tsx": 1,
  // The Pick dropdown's card-portaled catcher (`data-pick-backdrop`).
  "views/overlays/PickPortalMenu.tsx": 1,
};

function tsxFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = join(dir, e.name);
    if (e.isDirectory()) return e.name === "__tests__" ? [] : tsxFiles(p);
    return e.name.endsWith(".tsx") ? [p] : [];
  });
}

interface Hit {
  file: string;
  line: number;
  tag: string;
}

/** Every intrinsic, non-interactive JSX opening tag in `src` that carries onClick. */
function clickableStatics(file: string, src: string): Hit[] {
  const hits: Hit[] = [];
  const open = /<([a-z][a-z0-9]*)\b/g;
  let m: RegExpExecArray | null;
  while ((m = open.exec(src)) !== null) {
    const tag = m[1];
    if (INTERACTIVE.has(tag)) continue;
    // Walk to the tag's closing `>`, skipping `>` inside `{…}` expressions.
    let depth = 0;
    let i = m.index + m[0].length;
    for (; i < src.length; i++) {
      const c = src[i];
      if (c === "{") depth++;
      else if (c === "}") depth--;
      else if (c === ">" && depth === 0) break;
    }
    const attrs = src.slice(m.index, i);
    if (/\sonClick=/.test(attrs)) {
      hits.push({
        file,
        line: src.slice(0, m.index).split("\n").length,
        tag,
      });
    }
  }
  return hits;
}

describe("no onClick on non-interactive elements (#307)", () => {
  const files = tsxFiles(SRC);

  it("scans the source tree", () => {
    expect(files.length).toBeGreaterThan(50);
  });

  it("the scanner catches a multi-line clickable div and ignores a button", () => {
    const sample = [
      "<div",
      "  style={{ a: b > c ? 1 : 2 }}",
      "  onClick={() => {",
      "    go();",
      "  }}",
      ">",
      '<button type="button" onClick={go}>x</button>',
      "<span onClick={go} />",
    ].join("\n");
    expect(clickableStatics("x.tsx", sample).map((h) => h.tag)).toEqual([
      "div",
      "span",
    ]);
  });

  it("every clickable element outside the backdrop allowlist is a real button", () => {
    const offenders: string[] = [];
    for (const f of files) {
      const rel = relative(SRC, f).split("\\").join("/");
      const hits = clickableStatics(rel, readFileSync(f, "utf8"));
      const allowed = BACKDROP_ALLOWLIST[rel] ?? 0;
      if (hits.length > allowed) {
        offenders.push(
          ...hits.map((h) => `${h.file}:${String(h.line)} <${h.tag} onClick>`),
        );
      }
    }
    expect(offenders).toEqual([]);
  });
});
