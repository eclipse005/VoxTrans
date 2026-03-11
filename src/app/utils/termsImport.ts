import type { TermEntry } from "../types";

type ParsedRow = { source: string; target: string; note: string } | "comment" | null;

function splitRow(line: string): ParsedRow {
  if (line.startsWith("#") || line.startsWith("//")) return "comment";

  const parts = line.split("=").map((part) => part.trim());
  const source = parts[0] ?? "";
  const target = parts[1] ?? "";
  const note = parts.slice(2).join(" = ");
  return source && target ? { source, target, note } : null;
}

export function parseImportedTerms(input: string, existingTerms: TermEntry[]) {
  const rows = input
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);

  const existed = new Set(existingTerms.map((item) => item.source.toLowerCase()));
  const imported: TermEntry[] = [];
  let invalidCount = 0;
  let duplicateCount = 0;

  for (const line of rows) {
    const parsed = splitRow(line);
    if (parsed === "comment") continue;
    if (!parsed) {
      invalidCount += 1;
      continue;
    }

    const key = parsed.source.toLowerCase();
    if (existed.has(key)) {
      duplicateCount += 1;
      continue;
    }

    existed.add(key);
    imported.push({
      id: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
      source: parsed.source,
      target: parsed.target,
      note: parsed.note,
    });
  }

  return {
    imported,
    invalidCount,
    duplicateCount,
  };
}
