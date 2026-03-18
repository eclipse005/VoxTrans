import type { TerminologyGroup, TerminologyTerm } from "../../features/media/types";

export const DEFAULT_TERMINOLOGY_GROUP_NAME = "默认";

export function createTerminologyGroup(name?: string): TerminologyGroup {
  return {
    id: makeId("group"),
    name: (name ?? DEFAULT_TERMINOLOGY_GROUP_NAME).trim() || DEFAULT_TERMINOLOGY_GROUP_NAME,
    terms: [],
  };
}

export function createTerminologyTerm(
  origin: string,
  target: string,
  note?: string,
): TerminologyTerm {
  return {
    id: makeId("term"),
    origin: origin.trim(),
    target: target.trim(),
    note: (note ?? "").trim(),
  };
}

export function parseBatchTerminologyInput(input: string): {
  terms: TerminologyTerm[];
  skipped: number;
} {
  const lines = input
    .split(/\r?\n/)
    .flatMap((line) => line.split(","));
  const terms: TerminologyTerm[] = [];
  let skipped = 0;

  for (const rawLine of lines) {
    const line = rawLine.trim();
    if (!line) continue;
    const parts = splitLine(line);
    if (parts.length < 2) {
      skipped += 1;
      continue;
    }
    const origin = (parts[0] ?? "").trim();
    const target = (parts[1] ?? "").trim();
    const note = (parts[2] ?? "").trim();
    if (!origin || !target) {
      skipped += 1;
      continue;
    }
    terms.push(createTerminologyTerm(origin, target, note));
  }

  return { terms, skipped };
}

function splitLine(line: string): string[] {
  const parts = line.split(":");
  if (parts.length <= 3) return parts;
  return [parts[0], parts[1], parts.slice(2).join(":")];
}

function makeId(prefix: string): string {
  return `${prefix}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

export function parseInlineTerminologyInput(input: string): {
  terms: TerminologyTerm[];
  skipped: number;
} {
  return parseBatchTerminologyInput(input);
}

export function normalizeTerminologyGroups(groups: TerminologyGroup[]): TerminologyGroup[] {
  if (groups.length > 0) return groups;
  return [createTerminologyGroup(DEFAULT_TERMINOLOGY_GROUP_NAME)];
}
