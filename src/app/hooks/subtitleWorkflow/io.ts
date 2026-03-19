import { invoke } from "@tauri-apps/api/core";

import type { QueueItem, SubtitleCue } from "../../../features/media/types";
import { buildFallbackCue, cuesToSrt, parseSrtContent } from "../../../features/media/srt";
import { exportSrt } from "../../api/transcribe";

export async function saveSubtitleEditor(
  taskId: string,
  cues: SubtitleCue[],
): Promise<void> {
  const content = cuesToSrt(cues);
  const subtitleSegmentsJson = JSON.stringify(
    cues.map((cue) => ({
      startMs: Math.max(0, Math.round(cue.startMs)),
      endMs: Math.max(Math.round(cue.startMs), Math.round(cue.endMs)),
      sourceText: cue.text || "",
      translatedText: cue.translatedText || "",
    })),
  );
  return invoke<void>("save_subtitle_editor", {
    request: {
      taskId,
      content,
      subtitleSegmentsJson,
    },
  });
}

export async function loadSubtitleEditorData(item: QueueItem): Promise<{
  response: { srtPath: string; warnings: string[] };
  hydratedCues: SubtitleCue[];
}> {
  const content = (item.resultSrt || "").replace(/\r\n/g, "\n");
  const response = {
    srtPath: "",
    warnings: [],
  };

  const parsedCues = parseSrtContent(content);
  const fallbackFromSegments = buildFallbackCueFromSegments(item);
  const effectiveCues =
    parsedCues.length > 0
      ? parsedCues
      : fallbackFromSegments.length > 0
        ? fallbackFromSegments
        : buildFallbackCue(content);
  const hydratedCues = hydrateTranslatedCues(effectiveCues, item);
  return { response, hydratedCues };
}

export async function exportSubtitleToDirectory(
  taskId: string,
  targetDir: string,
  taskName: string,
  cues: SubtitleCue[],
): Promise<string> {
  const content = cuesToSrt(cues);
  return exportSrt({
    taskId,
    targetDir,
    taskName,
    content,
  });
}

function hydrateTranslatedCues(cues: SubtitleCue[], item: QueueItem): SubtitleCue[] {
  const segments = parseSubtitleSegments(item.subtitleSegmentsJson);
  if (segments.length === 0) return cues.map((cue) => ({ ...cue, translatedText: cue.translatedText || "" }));

  return cues.map((cue, index) => {
    const byIndex = segments[index];
    const byTime = segments.find((segment) => overlapMs(cue.startMs, cue.endMs, segment.startMs, segment.endMs) > 0);
    const sourceText = (byTime?.sourceText ?? byIndex?.sourceText ?? "").trim();
    const translatedText = (byTime?.translatedText ?? byIndex?.translatedText ?? "").trim();
    return {
      ...cue,
      text: sourceText || cue.text,
      translatedText: translatedText || cue.translatedText || "",
    };
  });
}

function parseSubtitleSegments(raw?: string): Array<{ startMs: number; endMs: number; sourceText: string; translatedText: string }> {
  if (!raw?.trim()) return [];
  try {
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed
      .map((segment) => {
        const start = Number(segment?.startMs ?? 0);
        const end = Number(segment?.endMs ?? start);
        const sourceText = typeof segment?.sourceText === "string" ? segment.sourceText : "";
        const translatedText = typeof segment?.translatedText === "string" ? segment.translatedText : "";
        if (!Number.isFinite(start) || !Number.isFinite(end)) return null;
        return {
          startMs: Math.max(0, Math.round(start)),
          endMs: Math.max(0, Math.round(end)),
          sourceText,
          translatedText,
        };
      })
      .filter((segment): segment is { startMs: number; endMs: number; sourceText: string; translatedText: string } => segment !== null);
  } catch {
    return [];
  }
}

function overlapMs(aStart: number, aEnd: number, bStart: number, bEnd: number): number {
  return Math.max(0, Math.min(aEnd, bEnd) - Math.max(aStart, bStart));
}

function buildFallbackCueFromSegments(item: QueueItem): SubtitleCue[] {
  const segments = parseSubtitleSegments(item.subtitleSegmentsJson);
  if (segments.length === 0) return [];
  return segments.map((segment, index) => ({
    id: `${item.id}-seg-${index}-${segment.startMs}-${segment.endMs}`,
    startMs: Math.max(0, segment.startMs),
    endMs: Math.max(segment.startMs, segment.endMs),
    text: segment.sourceText || "",
    translatedText: segment.translatedText || "",
  }));
}
