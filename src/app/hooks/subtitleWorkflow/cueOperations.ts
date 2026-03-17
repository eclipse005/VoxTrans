import type { SubtitleCue } from "../../../features/media/types";
import { createCueAfter } from "../../../features/media/srt";

export function updateCueList(cues: SubtitleCue[], cueId: string, patchCue: Partial<SubtitleCue>): SubtitleCue[] {
  return cues.map((cue) =>
    cue.id === cueId
      ? {
          ...cue,
          ...patchCue,
        }
      : cue,
  );
}

export function addCueAfterSelection(cues: SubtitleCue[], selectedCueId: string | null): SubtitleCue[] {
  const selectedIndex = selectedCueId ? cues.findIndex((cue) => cue.id === selectedCueId) : -1;
  if (selectedIndex < 0) {
    const lastCue = cues[cues.length - 1];
    const newCue = createCueAfter(lastCue);
    return [...cues, newCue];
  }

  const anchorCue = cues[selectedIndex];
  const newCue = createCueAfter(anchorCue);
  const next = [...cues];
  next.splice(selectedIndex + 1, 0, newCue);
  return next;
}

export function mergeSelectedCueList(cues: SubtitleCue[], selectedCueIds: string[]): SubtitleCue[] {
  const selectedSet = new Set(selectedCueIds);
  const selectedIndices = cues
    .map((cue, index) => ({ cue, index }))
    .filter(({ cue }) => selectedSet.has(cue.id));

  if (selectedIndices.length < 2) return cues;

  selectedIndices.sort((a, b) => a.index - b.index);
  const first = selectedIndices[0];
  const mergedText = selectedIndices
    .map(({ cue }) => cue.text.trim())
    .filter(Boolean)
    .join("\n");
  const mergedTranslatedText = selectedIndices
    .map(({ cue }) => cue.translatedText.trim())
    .filter(Boolean)
    .join("\n");

  const mergedCue: SubtitleCue = {
    ...first.cue,
    startMs: Math.min(...selectedIndices.map(({ cue }) => cue.startMs)),
    endMs: Math.max(...selectedIndices.map(({ cue }) => cue.endMs)),
    text: mergedText,
    translatedText: mergedTranslatedText,
  };

  const mergedIds = new Set(selectedIndices.map(({ cue }) => cue.id));
  const base = cues.filter((cue) => !mergedIds.has(cue.id));
  const insertAt = Math.min(first.index, base.length);
  return [...base.slice(0, insertAt), mergedCue, ...base.slice(insertAt)];
}

export function splitSelectedCueList(
  cues: SubtitleCue[],
  selectedCueIds: string[],
): { nextCues: SubtitleCue[]; bornCueIds: Array<{ sourceCueId: string; bornCueId: string }> } {
  if (!selectedCueIds.length) {
    return { nextCues: cues, bornCueIds: [] };
  }

  const selectedSet = new Set(selectedCueIds);
  const nextCues: SubtitleCue[] = [];
  const bornCueIds: Array<{ sourceCueId: string; bornCueId: string }> = [];

  for (const cue of cues) {
    if (!selectedSet.has(cue.id)) {
      nextCues.push(cue);
      continue;
    }

    const duration = Math.max(2, cue.endMs - cue.startMs);
    const middle = cue.startMs + Math.floor(duration / 2);
    const splitAt = Math.max(cue.startMs + 1, Math.min(cue.endMs - 1, middle));
    const [leftText, rightText] = splitCueText(cue.text);
    const [leftTranslatedText, rightTranslatedText] = splitCueText(cue.translatedText);

    const leftCue: SubtitleCue = {
      ...cue,
      id: `${cue.id}-a-${Math.random().toString(36).slice(2, 6)}`,
      startMs: cue.startMs,
      endMs: splitAt,
      text: leftText,
      translatedText: leftTranslatedText,
    };
    const rightCue: SubtitleCue = {
      ...cue,
      id: `${cue.id}-b-${Math.random().toString(36).slice(2, 6)}`,
      startMs: splitAt,
      endMs: cue.endMs,
      text: rightText,
      translatedText: rightTranslatedText,
    };

    bornCueIds.push({ sourceCueId: cue.id, bornCueId: rightCue.id });
    nextCues.push(leftCue, rightCue);
  }

  return { nextCues, bornCueIds };
}

export function replaceTextInCueList(
  cues: SubtitleCue[],
  findText: string,
  replaceText: string,
  scopeCueIds: string[] | null,
  maxReplacements?: number,
): { nextCues: SubtitleCue[]; replacedCount: number } {
  const source = findText;
  if (!source) return { nextCues: cues, replacedCount: 0 };

  const targetSet = scopeCueIds && scopeCueIds.length > 0 ? new Set(scopeCueIds) : null;
  const limit = maxReplacements && maxReplacements > 0 ? maxReplacements : Number.POSITIVE_INFINITY;
  let replacedCount = 0;

  const nextCues = cues.map((cue) => {
    if (targetSet && !targetSet.has(cue.id)) {
      return cue;
    }
    if (replacedCount >= limit) {
      return cue;
    }

    const text = cue.text;
    if (!text.includes(source)) {
      return cue;
    }

    let cursor = 0;
    let nextText = "";
    let localCount = 0;
    while (cursor < text.length && replacedCount + localCount < limit) {
      const index = text.indexOf(source, cursor);
      if (index < 0) break;
      nextText += text.slice(cursor, index) + replaceText;
      cursor = index + source.length;
      localCount += 1;
    }

    if (localCount === 0) {
      return cue;
    }

    nextText += text.slice(cursor);
    replacedCount += localCount;
    return {
      ...cue,
      text: nextText,
    };
  });

  return { nextCues, replacedCount };
}

export function removeCueFromList(cues: SubtitleCue[], cueId: string): SubtitleCue[] {
  return cues.filter((cue) => cue.id !== cueId);
}

function splitCueText(text: string): [string, string] {
  const trimmed = text.trim();
  if (!trimmed) return ["", ""];

  const words = trimmed.split(/\s+/).filter(Boolean);
  if (words.length >= 2) {
    const midWord = Math.floor(words.length / 2);
    return [words.slice(0, midWord).join(" "), words.slice(midWord).join(" ")];
  }

  const midChar = Math.max(1, Math.floor(trimmed.length / 2));
  return [trimmed.slice(0, midChar).trim(), trimmed.slice(midChar).trim()];
}
