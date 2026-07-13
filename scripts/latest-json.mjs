#!/usr/bin/env node
// Build the tauri-updater `latest.json` manifest from a fresh `tauri build`.
// Reads the single `*.app.tar.gz.sig` minisign signature the build produced and
// emits latest.json alongside it. semantic-release then uploads latest.json (and
// the .app.tar.gz) as release assets; the .sig is NOT uploaded (its content is
// embedded here).
//
// Usage: node scripts/latest-json.mjs <version> <notesBase64> [bundleDir]
// notesBase64 is base64-decoded into the `notes` field (empty string ⇒ "").
//
// ponytail: notes come in base64 (.releaserc prepareCmd runs
// `Buffer.from(nextRelease.notes).toString('base64')` in the exec template
// sandbox). If a CI run shows Buffer is unavailable in that sandbox, switch the
// prepareCmd to write the notes to a file and pass the path instead.

import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const [version, notesB64 = "", bundleDir = "src-tauri/target/universal-apple-darwin/release/bundle/macos"] =
  process.argv.slice(2);

if (!version) {
  console.error("usage: latest-json.mjs <version> <notesBase64> [bundleDir]");
  process.exit(1);
}

const sigs = readdirSync(bundleDir).filter((f) => f.endsWith(".app.tar.gz.sig"));
if (sigs.length !== 1) {
  console.error(
    `expected exactly one *.app.tar.gz.sig in ${bundleDir}, found ${sigs.length}: ${sigs.join(", ") || "(none)"}`,
  );
  process.exit(1);
}

const signature = readFileSync(join(bundleDir, sigs[0]), "utf8");
const notes = notesB64 ? Buffer.from(notesB64, "base64").toString("utf8") : "";

// The DMG/tarball is a universal binary, but the Tauri updater keys latest.json
// by the RUNNING slice's arch (compile-time `cfg!(target_arch)` per slice): an
// Apple Silicon install resolves `darwin-aarch64`, an Intel install resolves
// `darwin-x86_64`. There is no `darwin-universal` key. So both keys must point at
// the SAME universal tarball — omit `darwin-x86_64` and Intel installs run fine but
// never auto-update. One `entry` object under both keys so signature/url can't drift.
const entry = {
  signature,
  url: `https://github.com/pcavadas/tmp-companion/releases/download/v${version}/TMP-Companion-macOS.app.tar.gz`,
};

const manifest = {
  version,
  notes,
  pub_date: new Date().toISOString(),
  platforms: {
    "darwin-aarch64": entry,
    "darwin-x86_64": entry,
  },
};

const out = join(bundleDir, "latest.json");
writeFileSync(out, JSON.stringify(manifest, null, 2) + "\n");
console.log(`wrote ${out}`);
