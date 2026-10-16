#!/usr/bin/env node
// One gate for the checks AGENTS.md asks for before committing or pushing.
// There is no CI in this repo by choice, so this script is the machine part of
// "run the gate": each step mirrors a command spelled out in AGENTS.md, and
// stops at the first failure with the step name.
//
// Usage:
//   npm run verify         everything, including clippy and the Rust tests
//   npm run verify:fast    version + binaries + eslint + vitest + build only

import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const fast = process.argv.includes("--fast");

const steps = [
  { name: "version consistency", command: "node scripts/verify-version.mjs" },
  { name: "bundled binaries", command: "node scripts/verify-binaries.mjs" },
  { name: "eslint", command: "npm run lint" },
  { name: "vitest", command: "npm run test" },
  { name: "tsc + vite build", command: "npm run build" },
];

if (!fast) {
  steps.push(
    {
      name: "cargo clippy",
      command: "cargo clippy --workspace --all-targets -- -D warnings",
    },
    {
      // Two packages spelled out: `-p voxtrans` alone leaves voxtrans-core's
      // tests out of the run. `-j 1` matches how this suite is run by hand
      // (the engines are memory-hungry when linked and tested in parallel).
      name: "cargo test",
      command: "cargo test -j 1 -p voxtrans -p voxtrans-core",
    },
  );
}

const started = Date.now();
let failedAt = null;

steps.forEach((step, index) => {
  if (failedAt) return;
  console.log(`\n==> [${index + 1}/${steps.length}] ${step.name}`);
  const result = spawnSync(step.command, {
    cwd: root,
    stdio: "inherit",
    shell: true,
  });
  if (result.status !== 0) failedAt = step.name;
});

const seconds = Math.round((Date.now() - started) / 1000);
const label = fast ? "verify:fast" : "verify";

if (failedAt) {
  console.error(`\n${label} FAILED at: ${failedAt} (${seconds}s)`);
  process.exit(1);
}
console.log(`\n${label} OK — ${steps.length} steps in ${seconds}s`);
