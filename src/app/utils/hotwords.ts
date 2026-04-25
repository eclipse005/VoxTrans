import type { HotwordGroup, HotwordLang, HotwordTerm } from "../../features/media/types";

export const DEFAULT_HOTWORD_GROUP_NAME = "默认";

export function createHotwordGroup(name?: string): HotwordGroup {
  return {
    id: makeId("hotword-group"),
    name: (name ?? DEFAULT_HOTWORD_GROUP_NAME).trim() || DEFAULT_HOTWORD_GROUP_NAME,
    terms: [],
  };
}

export function createHotwordTerm(
  word: string,
  aliases: string[],
  lang: HotwordLang = "auto",
  note = "",
): HotwordTerm {
  return {
    id: makeId("hotword"),
    word: word.trim(),
    aliases: aliases.map((alias) => alias.trim()).filter(Boolean),
    lang,
    note: note.trim(),
  };
}

export function parseInlineHotwordInput(input: string): {
  terms: HotwordTerm[];
  skipped: number;
} {
  const entries = input
    .split(/\r?\n/)
    .flatMap((line) => line.split(/[;；]/));
  const terms: HotwordTerm[] = [];
  let skipped = 0;

  for (const rawEntry of entries) {
    const entry = rawEntry.trim();
    if (!entry) continue;
    const [rawWord, rawAliases = ""] = splitEntry(entry);
    const word = rawWord.trim();
    if (!word) {
      skipped += 1;
      continue;
    }
    const aliases = rawAliases
      .split(/[,，]/)
      .map((alias) => alias.trim())
      .filter(Boolean);
    terms.push(createHotwordTerm(word, aliases));
  }

  return { terms, skipped };
}

export function normalizeHotwordGroups(groups: HotwordGroup[]): HotwordGroup[] {
  if (groups.length > 0) return groups;
  return [createHotwordGroup(DEFAULT_HOTWORD_GROUP_NAME)];
}

function splitEntry(entry: string): [string, string] {
  const index = entry.indexOf("=");
  if (index < 0) return [entry, ""];
  return [entry.slice(0, index), entry.slice(index + 1)];
}

function makeId(prefix: string): string {
  return `${prefix}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}
