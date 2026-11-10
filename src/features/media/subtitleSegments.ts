import { cuesToSrt } from "./srt";
import type { SubtitleCue, SubtitleSegment, SubtitleWordAnchor } from "./types";
export function parseSubtitleSegments(raw?: string): SubtitleSegment[] {
  if (!raw?.trim()) return [];
  try {
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed
      .map((segment) => normalizeSubtitleSegment(segment))
      .filter((segment): segment is SubtitleSegment => segment !== null);
  } catch {
    return [];
  }
}

export function normalizeSubtitleSegmentsJson(raw?: string): string {
  return JSON.stringify(parseSubtitleSegments(raw));
}

export function subtitleSegmentsToSrt(segments: SubtitleSegment[]): string {
  return cuesToSrt(
    segments.map((segment, index) => ({
      id: `seg-${index}-${segment.startMs}-${segment.endMs}`,
      startMs: segment.startMs,
      endMs: segment.endMs,
      text: segment.sourceText,
      translatedText: segment.translatedText,
    })),
  );
}

export function buildSubtitleSegmentsFromCues(cues: SubtitleCue[]): SubtitleSegment[] {
  return cues.map((cue) => ({
    startMs: Math.max(0, Math.round(cue.startMs)),
    endMs: Math.max(Math.round(cue.startMs), Math.round(cue.endMs)),
    sourceText: cue.text || "",
    translatedText: cue.translatedText || "",
    sourceWords: [],
    semanticSentenceId: cue.semanticSentenceId ?? 0,
  }));
}

/**
 * Fold a beautified segment list back onto the editor's cue list.
 *
 * Only timing and translated text can change (see `services::subtitle_beautify`),
 * and the cue count is preserved, so ids stay stable and selection survives.
 * Returns `null` when the counts disagree — the caller must not guess.
 */
export function applyBeautifiedSegments(
  cues: SubtitleCue[],
  segments: SubtitleSegment[],
): SubtitleCue[] | null {
  if (segments.length !== cues.length) return null;
  return cues.map((cue, index) => ({
    ...cue,
    startMs: segments[index].startMs,
    endMs: segments[index].endMs,
    translatedText: segments[index].translatedText,
  }));
}

export function buildCueListFromSubtitleSegments(
  taskId: string,
  segments: SubtitleSegment[],
): SubtitleCue[] {
  return segments.map((segment, index) => ({
    id: `${taskId}-seg-${index}-${segment.startMs}-${segment.endMs}`,
    startMs: segment.startMs,
    endMs: segment.endMs,
    text: segment.sourceText || "",
    translatedText: segment.translatedText || "",
    semanticSentenceId: segment.semanticSentenceId ?? 0,
  }));
}

function normalizeSubtitleSegment(segment: unknown): SubtitleSegment | null {
  const startMs = Number((segment as { startMs?: unknown })?.startMs ?? 0);
  const endMs = Number((segment as { endMs?: unknown })?.endMs ?? startMs);
  if (!Number.isFinite(startMs) || !Number.isFinite(endMs)) return null;

  const sourceWords = Array.isArray((segment as { sourceWords?: unknown[] })?.sourceWords)
    ? ((segment as { sourceWords?: unknown[] }).sourceWords ?? [])
        .map((word) => normalizeSubtitleWordAnchor(word))
        .filter((word): word is SubtitleWordAnchor => word !== null)
    : [];

  return {
    startMs: Math.max(0, Math.round(startMs)),
    endMs: Math.max(0, Math.round(endMs)),
    sourceText: String((segment as { sourceText?: unknown })?.sourceText ?? ""),
    translatedText: String((segment as { translatedText?: unknown })?.translatedText ?? ""),
    sourceWords,
    semanticSentenceId: Math.max(
      0,
      Math.round(Number((segment as { semanticSentenceId?: unknown })?.semanticSentenceId ?? 0)) || 0,
    ),
  };
}

function normalizeSubtitleWordAnchor(word: unknown): SubtitleWordAnchor | null {
  const startMs = Number((word as { startMs?: unknown })?.startMs ?? 0);
  const endMs = Number((word as { endMs?: unknown })?.endMs ?? startMs);
  if (!Number.isFinite(startMs) || !Number.isFinite(endMs)) return null;
  return {
    startMs: Math.max(0, Math.round(startMs)),
    endMs: Math.max(0, Math.round(endMs)),
    word: String((word as { word?: unknown })?.word ?? ""),
  };
}
