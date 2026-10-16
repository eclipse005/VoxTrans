#!/usr/bin/env node
// Extracts the `## v<version>` section from github-release.md — the single
// source for user-facing release notes. The release workflow feeds the result
// to `gh release create`; a missing section fails the run on purpose, so a tag
// cannot ship without notes.
//
// Usage: node scripts/release-notes.mjs v1.5.1 [--out notes.md]

import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

const args = process.argv.slice(2);
const tag = args.find((arg) => !arg.startsWith("--"));
if (!tag) {
  console.error("usage: node scripts/release-notes.mjs v<version> [--out <file>]");
  process.exit(2);
}
const outIndex = args.indexOf("--out");
const outFile = outIndex !== -1 ? args[outIndex + 1] : null;
if (outIndex !== -1 && !outFile) {
  console.error("--out needs a file path");
  process.exit(2);
}
const version = tag.replace(/^v/, "");

const lines = readFileSync(path.join(root, "github-release.md"), "utf8").split(/\r?\n/);
const heading = new RegExp(`^##\\s+v${version.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\s*$`);
const start = lines.findIndex((line) => heading.test(line.trim()));
if (start === -1) {
  console.error(
    `github-release.md has no "## v${version}" section. Prepare the notes first ` +
      `(rename the 「未发布」 section) and land them in the same commit as the tag.`,
  );
  process.exit(1);
}
let end = lines.findIndex((line, index) => index > start && /^##\s/.test(line));
if (end === -1) end = lines.length;
// The file separates versions with `---`; the separator belongs to neither
// section's body.
const body =
  lines
    .slice(start + 1, end)
    .join("\n")
    .replace(/(\n\s*-{3,})+\s*$/, "")
    .trim() + "\n";

if (outFile) {
  writeFileSync(outFile, body);
  console.log(`release notes for v${version}: ${body.length} chars -> ${outFile}`);
} else {
  process.stdout.write(body);
}
