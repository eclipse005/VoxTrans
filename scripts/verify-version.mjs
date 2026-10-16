#!/usr/bin/env node
// Version consistency: package.json, src-tauri/Cargo.toml and
// src-tauri/tauri.conf.json must agree. build-release.ps1 authors all three in
// one go; this is the tripwire for the case where one of them is hand-edited,
// or a release is package from a tree where they drifted apart.
//
// Usage:
//   node scripts/verify-version.mjs                assert all three agree
//   node scripts/verify-version.mjs --print        print the version (asserts first)
//   node scripts/verify-version.mjs --expect 1.6.0 assert the version is exactly this
//   node scripts/verify-version.mjs --tag v1.6.0   assert tag and version agree

import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

const sources = [
  {
    label: "package.json",
    read: () => JSON.parse(readFileSync(path.join(root, "package.json"), "utf8")).version,
  },
  {
    label: "src-tauri/tauri.conf.json",
    read: () =>
      JSON.parse(readFileSync(path.join(root, "src-tauri", "tauri.conf.json"), "utf8"))
        .version,
  },
  {
    label: "src-tauri/Cargo.toml",
    read: () => {
      // The package-level version is the first `version = "..."` line; every
      // dependency keeps its own version inline on a crate-name line.
      const text = readFileSync(path.join(root, "src-tauri", "Cargo.toml"), "utf8");
      const match = text.match(/^version\s*=\s*"([^"]+)"/m);
      if (!match) throw new Error('no package-level `version = "..."` line found');
      return match[1];
    },
  },
];

function fail(message) {
  console.error(message);
  process.exit(1);
}

const argv = process.argv.slice(2);
const expectIndex = argv.indexOf("--expect");
const tagIndex = argv.indexOf("--tag");
const print = argv.includes("--print");

const found = [];
for (const source of sources) {
  let version;
  try {
    version = source.read();
  } catch (error) {
    fail(`verify-version: cannot read ${source.label}: ${error.message}`);
  }
  found.push({ label: source.label, version });
}

const distinct = [...new Set(found.map((entry) => entry.version))];
if (distinct.length !== 1) {
  console.error("verify-version: sources disagree:");
  for (const entry of found) console.error(`  ${entry.version}  ${entry.label}`);
  process.exit(1);
}
const version = distinct[0];

if (expectIndex !== -1) {
  const expected = argv[expectIndex + 1];
  if (!expected) fail("verify-version: --expect needs a value");
  if (version !== expected) {
    fail(`verify-version: expected ${expected}, found ${version}`);
  }
}

if (tagIndex !== -1) {
  const tag = argv[tagIndex + 1];
  if (!tag) fail("verify-version: --tag needs a value");
  const normalized = tag.replace(/^v/, "");
  if (normalized !== version) {
    fail(`verify-version: tag ${tag} does not match version ${version}`);
  }
}

if (print) {
  console.log(version);
} else {
  const labels = found.map((entry) => entry.label).join(", ");
  console.log(`verify-version: ${version} consistent across ${labels}`);
}
