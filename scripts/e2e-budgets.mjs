#!/usr/bin/env node
// Offline e2e timing budgets — ADVISORY. Sums each spec file's test durations out of
// Playwright's JSON report and warns about any file over its budget in
// scripts/e2e-budgets.tsv, and about any file the table has no row for. It never fails:
// wall times swing 2-4x between machines (playwright.config.ts's timeout note), so a hard
// gate on them would be a flaky one. Its job is to make a slowdown visible in the run log.
//
// A file's time is the sum of its tests' durations (every attempt, hooks included): the
// tests of one file run in order in one worker (`fullyParallel: false`), so the sum is that
// file's share of a worker, independent of how the files were spread across workers.
//
// Usage: bun scripts/e2e-budgets.mjs <report.json> <budgets.tsv>
// Exit: 0 always once both files are read (over-budget files are printed as warnings);
//       1 when the report or the budgets file cannot be read.

import { readFileSync } from "node:fs";
import { basename } from "node:path";

const [reportPath, budgetsPath] = process.argv.slice(2);
if (!reportPath || !budgetsPath) {
  console.error("usage: e2e-budgets.mjs <report.json> <budgets.tsv>");
  process.exit(1);
}

let report;
let budgetsText;
try {
  report = JSON.parse(readFileSync(reportPath, "utf8"));
  budgetsText = readFileSync(budgetsPath, "utf8");
} catch (e) {
  console.error(`e2e budgets: cannot read the timings (${e.message})`);
  process.exit(1);
}

// <spec file> TAB <seconds>; `#` starts a comment line.
const budgets = new Map();
for (const line of budgetsText.split("\n")) {
  if (!line.trim() || line.startsWith("#")) continue;
  const [file, secs] = line.split("\t");
  budgets.set(file.trim(), Number(secs));
}

// Sum every result's duration (ms) under a suite tree, by spec file.
const spent = new Map();
const walk = (suite) => {
  for (const spec of suite.specs ?? []) {
    const file = basename(spec.file ?? suite.file ?? "");
    for (const t of spec.tests ?? []) {
      for (const r of t.results ?? []) {
        spent.set(file, (spent.get(file) ?? 0) + (r.duration ?? 0));
      }
    }
  }
  for (const child of suite.suites ?? []) walk(child);
};
for (const s of report.suites ?? []) walk(s);

const YELLOW = "\x1b[33m";
const RESET = "\x1b[0m";
let over = 0;
const rows = [...spent.entries()].sort((a, b) => b[1] - a[1]);
console.log("\ne2e timing budgets (advisory) — spec file, seconds spent / budget:");
for (const [file, ms] of rows) {
  const secs = ms / 1000;
  const budget = budgets.get(file);
  if (budget === undefined) {
    console.log(`${YELLOW}⚠ ${file}: ${secs.toFixed(1)} s — no budget row in ${basename(budgetsPath)}${RESET}`);
    over++;
  } else if (secs > budget) {
    console.log(`${YELLOW}⚠ ${file}: ${secs.toFixed(1)} s OVER its ${String(budget)} s budget${RESET}`);
    over++;
  } else {
    console.log(`  ${file}: ${secs.toFixed(1)} s / ${String(budget)} s`);
  }
}
if (over > 0) {
  console.log(
    `${YELLOW}⚠ ${String(over)} spec file(s) over budget or unbudgeted — advisory only; ` +
      `find the slowdown, or re-measure and update ${budgetsPath}${RESET}`,
  );
}
