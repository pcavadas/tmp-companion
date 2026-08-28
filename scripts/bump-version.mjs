#!/usr/bin/env node
// Bump src-tauri/tauri.conf.json's `version` field in place. Shared by all three
// build legs of one release — the macOS job (via scripts/release-build.sh) and
// the Linux build-deb/build-rpm CI jobs (release.yml) — so they agree on the
// exact same version string instead of three copies of the same one-liner
// drifting out of sync.
import { readFileSync, writeFileSync } from "node:fs";

const version = process.argv[2];
if (!version) {
  console.error("usage: bump-version.mjs <version>");
  process.exit(1);
}

const f = "src-tauri/tauri.conf.json";
const j = JSON.parse(readFileSync(f, "utf8"));
j.version = version;
writeFileSync(f, JSON.stringify(j, null, 2) + "\n");
console.log(`bump-version: ${f} -> ${version}`);
