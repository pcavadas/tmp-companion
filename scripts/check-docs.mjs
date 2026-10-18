#!/usr/bin/env node
// check-docs.mjs — the docs link gate behind scripts/check-docs.sh (run it through that wrapper).
//
// Checks every tracked file, no network. Only tracked files are valid targets:
//   1. Markdown links `[x](path#anchor)` and reference definitions in *.md: the target exists
//      (relative to the linking file) and, for a *.md target, the anchor is a GitHub slug of
//      one of its headings (duplicate headings get -1, -2 …; `<a id|name>` anchors count).
//      Links inside fenced code blocks and http(s)/mailto links are ignored. In docs/*.html,
//      relative `href`/`src` values and `github.com/<owner>/tmp-companion/blob|tree/<ref>/…`
//      links into the repo get the same check.
//   2. Path mentions ending in `.md` (optionally `#anchor`), backticked, bold or bare, in *.md
//      prose, in the comments and strings of code, scripts, workflows and skill frontmatter,
//      and in *.json, *.tsv and docs/*.html: the file exists (resolved against the repo root,
//      then the mentioning file's directory, then as the tail of a tracked path) and the
//      anchor exists.
//   3. `--strict` (report-only, never changes the exit code): stale-wording ban list in prose
//      and comments — firmware 1.8.45 mentions, `#NNN` issue references in comments, and
//      plan labels such as `R5` or `AC3`.
//   4. Size caps, in bytes (SIZE_CAPS): AGENTS.md ≤ 10,240, CLAUDE.md ≤ 6,144,
//      .claude/rules/danger.md ≤ 8,192 and every notes/**/*.md ≤ 20,480.
//
// `--staged` (the pre-commit hook) reads every file from the index — the bytes the commit
// will hold — and reports only failures in staged files, unless the commit deletes or
// renames a file, which can break links anywhere: then the whole tree.
//
// Prints `file:line: message` per failure and exits 1 on any failure from checks 1, 2 and 4.
// Plain ESM on node builtins, so `node` or `bun` runs it unchanged.
import { execFileSync } from "node:child_process";
import { readFileSync, existsSync, statSync } from "node:fs";
import path from "node:path";

const strict = process.argv.includes("--strict");
const fromIndex = process.argv.includes("--staged");
const git = (...args) =>
  execFileSync("git", args, { encoding: "utf8", maxBuffer: 64 << 20 });
process.chdir(git("rev-parse", "--show-toplevel").trim());
// null = report every file; otherwise only these.
let only = null;
if (fromIndex) {
  const staged = (filter) =>
    git(
      "diff",
      "--cached",
      "--no-renames",
      "--name-only",
      "-z",
      `--diff-filter=${filter}`,
    )
      .split("\0")
      .filter(Boolean);
  if (staged("D").length === 0) only = new Set(staged("ACM"));
}

// Deliberate mentions of files that do not exist, as [file, mention]; "*" matches any.
// Justify every entry.
const ALLOW = [
  // The Mustang LT protocol write-up this lineage table compares against lives outside the repo.
  [
    ".claude/skills/tmp-companion-protocol/references/protocol-lineage.md",
    "Protocol.md",
  ],
  // Shell specs create these scratch files in a throwaway tree.
  ["scripts/gate-pr.spec.sh", "notes/x.md"],
  ["scripts/gate-pr.spec.sh", "notes/y.md"],
  ["scripts/e2e-lane.spec.sh", "notes/x.md"],
];
const allowed = (file, mention) =>
  ALLOW.some(
    ([f, m]) => (f === "*" || f === file) && (m === "*" || m === mention),
  );

const ls = (args) =>
  git("ls-files", "-z", ...args)
    .split("\0")
    .filter(Boolean);
// The index under --staged; otherwise the tracked files present in the working tree.
const tracked = fromIndex ? ls([]) : ls([]).filter((f) => existsSync(f));
const known = new Set(tracked);
const dirs = new Set();
for (const f of known) {
  let d = path.posix.dirname(f);
  while (d !== "." && !dirs.has(d)) {
    dirs.add(d);
    d = path.posix.dirname(d);
  }
}
const byBase = new Map();
for (const f of known) {
  const b = path.posix.basename(f);
  if (!byBase.has(b)) byBase.set(b, []);
  byBase.get(b).push(f);
}
const isFile = (p) => known.has(p);
const isDir = (p) => p === "" || p === "." || dirs.has(p);

// Under --staged, the index copy of every tracked text file (one `git cat-file --batch`).
const SCANNED = /\.(md|rs|ts|tsx|mjs|js|sh|yml|yaml|py|toml|json|tsv|html)$/;
const MAX_BYTES = 4 << 20;
const indexText = new Map();
if (fromIndex) {
  const blobs = git("ls-files", "-s", "-z")
    .split("\0")
    .filter(Boolean)
    .map((e) => /^(\d+) ([0-9a-f]+) \d+\t(.*)$/s.exec(e))
    .filter((m) => m && m[1] !== "160000" && SCANNED.test(m[3]));
  const out = execFileSync("git", ["cat-file", "--batch"], {
    input: blobs.map((m) => m[2]).join("\n") + "\n",
    maxBuffer: 256 << 20,
  });
  let pos = 0;
  for (const m of blobs) {
    const nl = out.indexOf(10, pos);
    const size = Number(out.toString("latin1", pos, nl).split(" ")[2]);
    pos = nl + 1;
    if (size <= MAX_BYTES)
      indexText.set(m[3], out.toString("utf8", pos, pos + size));
    pos += size + 1;
  }
}
// A file's text (index copy under --staged), or null when unreadable or over MAX_BYTES.
function readText(file) {
  if (fromIndex) return indexText.get(file) ?? null;
  try {
    if (statSync(file).size > MAX_BYTES) return null;
    return readFileSync(file, "utf8");
  } catch {
    return null;
  }
}

const failures = [];
const warnings = [];
const fail = (file, line, msg) => failures.push(`${file}:${line}: ${msg}`);
const warn = (file, line, msg) => warnings.push(`${file}:${line}: ${msg}`);

// ── headings → anchors ────────────────────────────────────────────────────
function slugBase(text) {
  // Strip tags to a fixed point: one pass can leave a tag spliced from the remains.
  let s = text;
  for (let prev; prev !== s;) {
    prev = s;
    s = s.replace(/<[^>]+>/g, "");
  }
  return s
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/`/g, "")
    .replace(/\*\*|__|\*/g, "")
    .trim()
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\p{M}\s_-]/gu, "")
    .replace(/\s/g, "-");
}

// Lines of a markdown file, with fenced code and YAML frontmatter flagged.
function mdLines(text) {
  const lines = text.split("\n");
  const out = [];
  let fence = null;
  let front = lines[0] === "---";
  for (let i = 0; i < lines.length; i++) {
    const l = lines[i];
    if (front) {
      out.push({ l, code: false, front: true });
      if (i > 0 && l === "---") front = false;
      continue;
    }
    const m = /^\s{0,3}(`{3,}|~{3,})(.*)$/.exec(l);
    if (fence) {
      out.push({ l, code: true });
      if (
        m &&
        m[1][0] === fence[0] &&
        m[1].length >= fence.length &&
        !/\S/.test(m[2])
      )
        fence = null;
      continue;
    }
    if (m) {
      fence = m[1];
      out.push({ l, code: true });
      continue;
    }
    out.push({ l, code: false });
  }
  return out;
}

const anchorCache = new Map();
function anchorsOf(file) {
  if (anchorCache.has(file)) return anchorCache.get(file);
  const set = new Set();
  const seen = new Map();
  // A missing target is reported by the caller.
  const text = readText(file) ?? "";
  for (const { l, code, front } of mdLines(text)) {
    if (code || front) continue;
    const h = /^\s{0,3}#{1,6}\s+(.*?)(?:\s+#+)?\s*$/.exec(l);
    if (h) {
      const base = slugBase(h[1]);
      const n = seen.get(base) ?? 0;
      seen.set(base, n + 1);
      set.add(n === 0 ? base : `${base}-${n}`);
    }
    for (const a of l.matchAll(
      /<a\s+(?:[^>]*\s)?(?:id|name)\s*=\s*["']([^"']+)["']/gi,
    ))
      set.add(a[1]);
  }
  anchorCache.set(file, set);
  return set;
}

function checkAnchor(file, line, target, anchor, mention) {
  if (!anchor || !target.endsWith(".md")) return;
  let a = anchor;
  try {
    a = decodeURIComponent(anchor);
  } catch {
    /* keep raw */
  }
  if (!anchorsOf(target).has(a.toLowerCase()))
    fail(file, line, `missing anchor #${anchor} in ${target} (${mention})`);
}

// ── 1. markdown links ─────────────────────────────────────────────────────
const LINK =
  /!?\[(?:[^\]\\]|\\.)*\]\(\s*(<[^>]*>|[^)\s]+)(?:\s+["'(][^)]*)?\)/g;
const REFDEF = /^\s{0,3}\[[^\]]+\]:\s*(<[^>]*>|\S+)/;

function checkLink(file, line, raw) {
  const t = raw.replace(/^<|>$/g, "");
  if (/^[a-z][a-z0-9+.-]*:/i.test(t) || t.startsWith("//")) return; // http(s), mailto, …
  const hash = t.indexOf("#");
  const p = hash >= 0 ? t.slice(0, hash) : t;
  const anchor = hash >= 0 ? t.slice(hash + 1) : "";
  let target = file;
  if (p !== "") {
    let dec = p;
    try {
      dec = decodeURIComponent(p);
    } catch {
      /* keep raw */
    }
    target = path.posix
      .normalize(
        dec.startsWith("/")
          ? dec.slice(1)
          : path.posix.join(path.posix.dirname(file), dec),
      )
      .replace(/\/$/, "");
    if (allowed(file, p)) return;
    if (target.startsWith(".."))
      return fail(file, line, `link leaves the repo: ${t}`);
    if (!isFile(target) && !isDir(target))
      return fail(file, line, `broken link: ${t}`);
  }
  checkAnchor(file, line, target, anchor, t);
}

// ── 2. .md path mentions ──────────────────────────────────────────────────
// No path or template char may precede a mention (so `/abs/x.md`, `${d}/x.md`, `*x.md` and
// `~/x.md` are skipped), and no word char may follow `.md`. Bold `**x.md**` is a mention.
const MENTION =
  /(?<![\w./\-$}{%~\\])(?<!(?<!\*)\*)((?:\.{1,2}\/)*(?:[\w.-]+\/)*[\w.-]*\w\.md)(?![\w-])(?:#([\w-]+))?/g;

function resolveMention(file, p) {
  const here = path.posix.dirname(file);
  if (/^\.\.?\//.test(p)) {
    // Relative to the file or to the directory a tool runs it from (e.g. the crate root
    // for `src-tauri/src/*.rs`): try the file's directory and each ancestor.
    for (let d = here; ; d = path.posix.dirname(d)) {
      const c = path.posix.normalize(path.posix.join(d, p));
      if (!c.startsWith("..") && isFile(c)) return c;
      if (d === ".") return null;
    }
  }
  for (const c of [path.posix.normalize(p), path.posix.join(here, p)])
    if (isFile(c)) return c;
  // A shorthand such as `danger.md` or `rules/danger.md`: the tail of a known path.
  const hits = (byBase.get(path.posix.basename(p)) ?? []).filter((f) =>
    f.endsWith(`/${p}`),
  );
  if (hits.length === 1) return hits[0];
  if (hits.length > 1) return hits.find((f) => f.startsWith(`${here}/`)) ?? "";
  return null;
}

function checkMentions(file, lineNo, text) {
  const clean = text.replace(/\b[a-z][a-z0-9+.-]*:\/\/\S+/gi, " ");
  for (const m of clean.matchAll(MENTION)) {
    const p = m[1];
    if (allowed(file, p)) continue;
    const target = resolveMention(file, p);
    if (target === null) {
      fail(file, lineNo, `missing file: ${p}${m[2] ? `#${m[2]}` : ""}`);
      continue;
    }
    if (target === "") continue; // ambiguous shorthand: the file exists, the anchor is unchecked
    checkAnchor(file, lineNo, target, m[2], m[0]);
  }
}

// ── 3. strict ban list (report-only) ──────────────────────────────────────
const BANS = [
  [/\bfw\s*1\.8\.45\b/i, "firmware 1.8.45 mention"],
  [/(?<![\w&])#[0-9]{2,4}\b/, "issue/PR number"],
  [/\b(?:AC|PR|P|T|D|R|E)[0-9]\b/, "plan label"],
];
function checkBans(file, lineNo, prose) {
  for (const [re, what] of BANS) {
    const m = re.exec(prose);
    if (m) warn(file, lineNo, `${what}: ${m[0]}`);
  }
}

// The comment part of a code line, or "" when it has none.
function commentOf(ext, l) {
  if (/^(sh|yml|yaml|py|toml)$/.test(ext)) {
    const m = /(?:^|\s)#(?![!{[])(.*)$/.exec(l);
    return m ? m[1] : "";
  }
  const t = l.trimStart();
  if (t.startsWith("*") || t.startsWith("/*")) return t;
  const i = l.search(/(?<!:)\/\//);
  return i >= 0 ? l.slice(i + 2) : "";
}

// ── docs/*.html links ─────────────────────────────────────────────────────
const ATTR = /\b(?:href|src)\s*=\s*["']([^"']*)["']/gi;
const REPO_URL =
  /^https:\/\/github\.com\/[^/]+\/tmp-companion\/(?:blob|tree)\/[^/]+\/([^?#]*)(#.*)?$/;
function checkHtmlLinks(file, line, l) {
  for (const [, v] of l.matchAll(ATTR)) {
    const repo = REPO_URL.exec(v);
    if (repo) checkLink(file, line, `/${repo[1]}${repo[2] ?? ""}`);
    else if (!v.startsWith("#")) checkLink(file, line, v);
  }
}

// ── 4. size caps ──────────────────────────────────────────────────────────
// [test, cap in bytes]; the first matching entry applies.
const SIZE_CAPS = [
  [(f) => f === "AGENTS.md", 10240],
  [(f) => f === "CLAUDE.md", 6144],
  [(f) => f === ".claude/rules/danger.md", 8192],
  [(f) => f.startsWith("notes/") && f.endsWith(".md"), 20480],
];
function checkSize(file) {
  const cap = SIZE_CAPS.find(([test]) => test(file))?.[1];
  if (cap === undefined) return;
  let size;
  if (fromIndex) {
    const text = indexText.get(file);
    // indexText skips only a blob over MAX_BYTES, which is far over every cap.
    size = text === undefined ? MAX_BYTES + 1 : Buffer.byteLength(text, "utf8");
  } else {
    try {
      size = statSync(file).size;
    } catch {
      return;
    }
  }
  if (size > cap) fail(file, 1, `${size} bytes, over the ${cap}-byte cap`);
}

// ── scan ──────────────────────────────────────────────────────────────────
const CODE_EXT = /^(rs|ts|tsx|mjs|js|sh|yml|yaml|py|toml)$/;
const DATA_EXT = /^(json|tsv)$/;
for (const file of tracked) {
  const ext = path.posix.extname(file).slice(1);
  const isMd = ext === "md";
  const isHtml = ext === "html" && file.startsWith("docs/");
  if (
    (!isMd && !isHtml && !CODE_EXT.test(ext) && !DATA_EXT.test(ext)) ||
    file === "scripts/check-docs.mjs"
  )
    continue;
  if (only && !only.has(file)) continue;
  checkSize(file);
  const text = readText(file);
  if (text === null) continue;
  if (isMd) {
    mdLines(text).forEach(({ l, code }, i) => {
      // Link syntax is checked outside fenced code only; a fence's bare paths still count.
      const rest = l.replace(LINK, " ").replace(REFDEF, " ");
      if (!code) {
        for (const m of l.matchAll(LINK)) checkLink(file, i + 1, m[1]);
        const d = REFDEF.exec(l);
        if (d) checkLink(file, i + 1, d[1]);
        if (strict) checkBans(file, i + 1, rest.replace(/`[^`]*`/g, " "));
      }
      checkMentions(file, i + 1, rest);
    });
  } else {
    text.split("\n").forEach((l, i) => {
      if (isHtml) checkHtmlLinks(file, i + 1, l);
      checkMentions(file, i + 1, l);
      if (strict && CODE_EXT.test(ext)) {
        const c = commentOf(ext, l);
        if (c) checkBans(file, i + 1, c);
      }
    });
  }
}

for (const w of warnings) console.log(`warn ${w}`);
if (strict)
  console.log(`check-docs --strict: ${warnings.length} report-only finding(s)`);
if (failures.length) {
  for (const f of failures) console.error(f);
  console.error(`check-docs: ${failures.length} failure(s)`);
  process.exit(1);
}
console.log("check-docs: clean");
