#!/usr/bin/env node
// Checks src-tauri/bin against scripts/binaries.sha256. These files ship inside
// the installer, so a swapped or truncated download must fail here before it
// reaches a build. Files that are not downloaded yet are skipped with a note
// (a fresh clone has no binaries and is not an error).
//
// Usage:
//   node scripts/verify-binaries.mjs    verify what is present
//   node scripts/verify-binaries.mjs --print   print current digests in
//                                              manifest format (for updates)

import { createHash } from "node:crypto";
import { readFileSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const manifestPath = path.join(root, "scripts", "binaries.sha256");
const binDir = path.join(root, "src-tauri", "bin");

function sha256(file) {
  return createHash("sha256").update(readFileSync(file)).digest("hex");
}

const entries = readFileSync(manifestPath, "utf8")
  .split(/\r?\n/)
  .map((line) => line.trim())
  .filter((line) => line && !line.startsWith("#"))
  .map((line) => {
    const match = line.match(/^([0-9a-fA-F]{64})\s+(\S+)$/);
    if (!match) {
      console.error(`verify-binaries: cannot parse manifest line: ${line}`);
      process.exit(1);
    }
    return { expected: match[1].toLowerCase(), name: match[2] };
  });

const print = process.argv.includes("--print");
let checked = 0;
let skipped = 0;
const problems = [];

for (const entry of entries) {
  const file = path.join(binDir, entry.name);
  let present = true;
  try {
    statSync(file);
  } catch {
    present = false;
  }
  if (!present) {
    skipped += 1;
    if (!print) console.log(`verify-binaries: skip ${entry.name} (not downloaded)`);
    continue;
  }
  const actual = sha256(file);
  if (print) {
    console.log(`${actual}  ${entry.name}`);
    continue;
  }
  checked += 1;
  if (actual !== entry.expected) {
    problems.push(`  ${entry.name}: expected ${entry.expected}, got ${actual}`);
  }
}

if (print) process.exit(0);

if (problems.length > 0) {
  console.error("verify-binaries: digest mismatch against scripts/binaries.sha256:");
  for (const problem of problems) console.error(problem);
  console.error(
    "If this was a deliberate update, replace the digests in the manifest in the same commit.",
  );
  process.exit(1);
}

console.log(
  `verify-binaries: ${checked} file(s) match scripts/binaries.sha256` +
    (skipped ? `, ${skipped} not downloaded` : ""),
);
