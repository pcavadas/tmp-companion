// src/__tests__/viewsModuleShape.test.ts — non-regression gate for #313: the views tree
// keeps ONE exported component per file and NO import cycles between its modules.
//
// Both checks parse every non-test `src/views/**/*.{ts,tsx}` source with the TypeScript
// compiler API (no regex guessing about JSX):
//
// - A COMPONENT export is an exported value whose name is PascalCase with at least one
//   lowercase letter (so `ROW_MIN_HEIGHT` and `OUTCOME_META` are constants, not
//   components) and whose declaration is a function, an arrow/function expression, a
//   call (`memo(…)`, `forwardRef(…)`), or a class. Type-only exports never count.
//   A value re-export (`export { Foo } from "./x"`) counts when `Foo` is a component
//   in `./x`. Barrels (`index.ts`) re-export and are skipped; a file may keep any
//   number of private (non-exported) helpers.
// - An IMPORT edge is any relative `import` / `export … from` between two views modules,
//   type-only ones included: a type-level cycle is still a cycle (the `EditMode` one
//   between `BlockEditor` and `validateBlockEdit` was exactly that).

import { existsSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import ts from "typescript";
import { describe, expect, it } from "vitest";

const VIEWS = join(process.cwd(), "src", "views");

function sources(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = join(dir, e.name);
    if (e.isDirectory()) return e.name === "__tests__" ? [] : sources(p);
    const isSource =
      /\.(ts|tsx)$/.test(e.name) && !/\.test\.(ts|tsx)$/.test(e.name);
    return isSource ? [p] : [];
  });
}

function parse(file: string): ts.SourceFile {
  return ts.createSourceFile(
    file,
    readFileSync(file, "utf8"),
    ts.ScriptTarget.Latest,
    true,
  );
}

const isComponentName = (name: string) =>
  /^[A-Z]/.test(name) && /[a-z]/.test(name);

function isExported(node: ts.Node): boolean {
  return (
    ts.canHaveModifiers(node) &&
    (ts.getModifiers(node) ?? []).some(
      (m) => m.kind === ts.SyntaxKind.ExportKeyword,
    )
  );
}

function isComponentInit(init: ts.Expression | undefined): boolean {
  if (!init) return false;
  return (
    ts.isArrowFunction(init) ||
    ts.isFunctionExpression(init) ||
    ts.isCallExpression(init) ||
    ts.isClassExpression(init)
  );
}

/** The component names a module exports: declared exports, local `export { … }`, and
 * value re-exports `export { Foo } from "./x"` of a name that IS a component in `./x`. */
function componentExports(
  sf: ts.SourceFile,
  seen = new Set<string>([sf.fileName]),
): string[] {
  const out: string[] = [];
  const localComponents = new Set<string>();
  for (const st of sf.statements) {
    if (ts.isFunctionDeclaration(st) || ts.isClassDeclaration(st)) {
      const name = st.name?.text;
      if (name && isComponentName(name)) {
        if (isExported(st)) out.push(name);
        else localComponents.add(name);
      }
    } else if (ts.isVariableStatement(st)) {
      for (const d of st.declarationList.declarations) {
        if (!ts.isIdentifier(d.name)) continue;
        const name = d.name.text;
        if (!isComponentName(name) || !isComponentInit(d.initializer)) continue;
        if (isExported(st)) out.push(name);
        else localComponents.add(name);
      }
    }
  }
  // `export { Foo }` / `export { Foo as Bar }` of a local component, no `from`.
  for (const st of sf.statements) {
    if (
      ts.isExportDeclaration(st) &&
      !st.moduleSpecifier &&
      !st.isTypeOnly &&
      st.exportClause &&
      ts.isNamedExports(st.exportClause)
    ) {
      for (const el of st.exportClause.elements) {
        const local = (el.propertyName ?? el.name).text;
        if (!el.isTypeOnly && localComponents.has(local))
          out.push(el.name.text);
      }
    }
  }
  // `export { Foo, Bar } from "./x"`: resolve `./x` and count the names that are
  // components there (cycle-guarded by `seen`).
  for (const st of sf.statements) {
    if (
      !ts.isExportDeclaration(st) ||
      st.isTypeOnly ||
      !st.moduleSpecifier ||
      !ts.isStringLiteral(st.moduleSpecifier) ||
      !st.exportClause ||
      !ts.isNamedExports(st.exportClause)
    )
      continue;
    const target = resolveImport(sf.fileName, st.moduleSpecifier.text);
    if (target === null || seen.has(target)) continue;
    seen.add(target);
    const there = new Set(componentExports(parse(target), seen));
    for (const el of st.exportClause.elements) {
      if (!el.isTypeOnly && there.has((el.propertyName ?? el.name).text))
        out.push(el.name.text);
    }
  }
  return out;
}

function resolveImport(from: string, spec: string): string | null {
  if (!spec.startsWith(".")) return null;
  const base = resolve(dirname(from), spec);
  for (const cand of [
    `${base}.ts`,
    `${base}.tsx`,
    join(base, "index.ts"),
    join(base, "index.tsx"),
  ]) {
    if (existsSync(cand)) return cand;
  }
  return null;
}

/** Relative module edges out of `sf`, kept only when they land inside src/views. */
function viewsImports(file: string, sf: ts.SourceFile): string[] {
  const out: string[] = [];
  for (const st of sf.statements) {
    const spec =
      (ts.isImportDeclaration(st) || ts.isExportDeclaration(st)) &&
      st.moduleSpecifier &&
      ts.isStringLiteral(st.moduleSpecifier)
        ? st.moduleSpecifier.text
        : null;
    if (spec === null) continue;
    const target = resolveImport(file, spec);
    if (target?.startsWith(VIEWS) && !/\.test\.(ts|tsx)$/.test(target))
      out.push(target);
  }
  return out;
}

/** Every elementary cycle's first-found witness, via an iterative-colour DFS. */
function findCycles(graph: Map<string, string[]>): string[][] {
  const WHITE = 0;
  const GREY = 1;
  const BLACK = 2;
  const colour = new Map<string, number>();
  const stack: string[] = [];
  const cycles: string[][] = [];
  const visit = (n: string) => {
    colour.set(n, GREY);
    stack.push(n);
    for (const m of graph.get(n) ?? []) {
      const c = colour.get(m) ?? WHITE;
      if (c === GREY) cycles.push([...stack.slice(stack.indexOf(m)), m]);
      else if (c === WHITE) visit(m);
    }
    stack.pop();
    colour.set(n, BLACK);
  };
  for (const n of graph.keys())
    if ((colour.get(n) ?? WHITE) === WHITE) visit(n);
  return cycles;
}

describe("src/views module shape (#313)", () => {
  const files = sources(VIEWS);
  const parsed = new Map(files.map((f) => [f, parse(f)]));
  const rel = (f: string) => relative(VIEWS, f);

  it("scans the views tree", () => {
    expect(files.length).toBeGreaterThan(50);
  });

  it("the component detector sees a known component export", () => {
    const sf = parsed.get(join(VIEWS, "PresetList.tsx"));
    expect(sf && componentExports(sf)).toEqual(["PresetList"]);
  });

  it("the component detector counts a value re-export from another module", () => {
    // `export { DialogCardCtx } from "../../ui/dialogContext"` — a re-exported
    // component must count toward the one-per-file cap, not slip past it.
    const sf = parsed.get(join(VIEWS, "overlays", "wizardContext.ts"));
    expect(sf && componentExports(sf)).toContain("DialogCardCtx");
  });

  it("no views file exports more than one component", () => {
    const offenders = files
      .filter((f) => !/[\\/]index\.tsx?$/.test(f))
      .map((f) => {
        const sf = parsed.get(f);
        return { file: rel(f), comps: sf ? componentExports(sf) : [] };
      })
      .filter((o) => o.comps.length > 1)
      .map((o) => `${o.file}: ${o.comps.join(", ")}`);
    expect(offenders).toEqual([]);
  });

  it("no import cycles among views modules", () => {
    const graph = new Map<string, string[]>();
    for (const [f, sf] of parsed) graph.set(f, viewsImports(f, sf));
    const cycles = findCycles(graph).map((c) => c.map(rel).join(" -> "));
    expect(cycles).toEqual([]);
  });
});
