import { describe, expect, it } from "vitest";

/// Both language bundles ship together; a key that exists on one side only is
/// invisible until a user switches language and reads an i18next key name.
/// Same for a `{{count}}` that got renamed on one side only: the string still
/// renders, just without the number. Neither shows up in types, lint or build,
/// so the shapes are pinned here. A namespace file added to one side only shows
/// up as a missing entry below, a namespace added to both needs no change here.

type Language = "zh-CN" | "en";

// Eager glob instead of fs: this keeps `tsc -b` (which typechecks tests under
// tsconfig.app.json, a browser config without Node types) happy, and vitest
// resolves it the same way the app build does.
const modules = import.meta.glob<Record<string, unknown>>("./locales/*/*.json", {
  eager: true,
  import: "default",
});

function bundleMap(language: Language): Map<string, Record<string, unknown>> {
  const files = new Map<string, Record<string, unknown>>();
  for (const [modulePath, bundle] of Object.entries(modules)) {
    const match = modulePath.match(/^\.\/locales\/([^/]+)\/([^/]+\.json)$/);
    if (!match || match[1] !== language) continue;
    files.set(match[2], bundle);
  }
  return files;
}

const bundles: Record<Language, Map<string, Record<string, unknown>>> = {
  "zh-CN": bundleMap("zh-CN"),
  en: bundleMap("en"),
};

const namespaceFiles = [...bundles["zh-CN"].keys()].sort();

type Leaf = {
  path: string;
  value: unknown;
};

function collectLeaves(node: unknown, prefix: string, out: Leaf[]): Leaf[] {
  if (node !== null && typeof node === "object" && !Array.isArray(node)) {
    for (const [key, value] of Object.entries(node as Record<string, unknown>)) {
      collectLeaves(value, prefix ? `${prefix}.${key}` : key, out);
    }
    return out;
  }
  out.push({ path: prefix, value: node });
  return out;
}

function leavesOf(bundle: Record<string, unknown>): Map<string, unknown> {
  const leaves: Leaf[] = [];
  collectLeaves(bundle, "", leaves);
  return new Map(leaves.map((leaf) => [leaf.path, leaf.value]));
}

function placeholders(value: unknown): string[] {
  if (typeof value !== "string") return [];
  return [...value.matchAll(/\{\{\s*([^{}]+?)\s*\}\}/g)]
    .map((match) => match[1].trim())
    .sort();
}

describe("i18n locale bundles", () => {
  it("both languages ship the same namespace files", () => {
    const englishFiles = [...bundles.en.keys()].sort();
    expect(englishFiles).toEqual(namespaceFiles);
  });

  it.each(namespaceFiles)(
    "%s: leaf keys, value shapes and placeholders match across languages",
    (file: string) => {
      const zhCN = leavesOf(bundles["zh-CN"].get(file) ?? {});
      const en = leavesOf(bundles.en.get(file) ?? {});
      const problems: string[] = [];

      for (const key of zhCN.keys()) {
        if (!en.has(key)) problems.push(`missing in en: ${key}`);
      }
      for (const key of en.keys()) {
        if (!zhCN.has(key)) problems.push(`missing in zh-CN: ${key}`);
      }
      for (const [key, value] of zhCN) {
        if (!en.has(key)) continue;
        const counterpart = en.get(key);
        if (typeof counterpart !== typeof value) {
          problems.push(
            `type mismatch at ${key}: ${typeof value} vs ${typeof counterpart}`,
          );
          continue;
        }
        const expected = placeholders(value).join(",");
        const actual = placeholders(counterpart).join(",");
        if (expected !== actual) {
          problems.push(
            `placeholder mismatch at ${key}: [${expected}] vs [${actual}]`,
          );
        }
      }

      expect(problems).toEqual([]);
    },
  );
});
