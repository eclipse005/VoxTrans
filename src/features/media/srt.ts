import type { SubtitleCue } from "./types";

const TIME_RE = /^(\d{2}):(\d{2}):(\d{2}),(\d{3})$/;
const RANGE_RE = /^(.+?)\s*-->\s*(.+)$/;

export function formatSrtTime(ms: number): string {
  const safe = Math.max(0, Math.round(ms));
  const hours = Math.floor(safe / 3_600_000);
  const minutes = Math.floor((safe % 3_600_000) / 60_000);
  const seconds = Math.floor((safe % 60_000) / 1_000);
  const millis = safe % 1_000;
  return `${hours.toString().padStart(2, "0")}:${minutes.toString().padStart(2, "0")}:${seconds
    .toString()
    .padStart(2, "0")},${millis.toString().padStart(3, "0")}`;
}

export function parseSrtTime(value: string): number | null {
  const m = TIME_RE.exec(value.trim());
  if (!m) return null;
  const h = Number.parseInt(m[1], 10);
  const min = Number.parseInt(m[2], 10);
  const sec = Number.parseInt(m[3], 10);
  const ms = Number.parseInt(m[4], 10);
  if (min >= 60 || sec >= 60 || ms >= 1000) return null;
  return h * 3_600_000 + min * 60_000 + sec * 1_000 + ms;
}

export function parseSrtContent(content: string): SubtitleCue[] {
  const normalized = content.replace(/\r\n/g, "\n");
  const blocks = normalized
    .split(/\n{2,}/)
    .map((block) => block.trim())
    .filter(Boolean);

  const cues: SubtitleCue[] = [];
  for (let i = 0; i < blocks.length; i += 1) {
    const lines = blocks[i].split("\n");
    if (!lines.length) continue;

    let lineOffset = 0;
    if (/^\d+$/.test(lines[0].trim())) {
      lineOffset = 1;
    }

    const tsLine = lines[lineOffset]?.trim();
    if (!tsLine) continue;
    const range = RANGE_RE.exec(tsLine);
    if (!range) continue;

    const startMs = parseSrtTime(range[1]);
    const endMs = parseSrtTime(range[2]);
    if (startMs == null || endMs == null) continue;

    const text = lines.slice(lineOffset + 1).join("\n").trim();
    cues.push({
      id: cueId(i + 1, startMs),
      startMs,
      endMs: Math.max(endMs, startMs),
      text,
      translatedText: "",
    });
  }

  return cues;
}

export function cuesToSrt(cues: SubtitleCue[]): string {
  return cues
    .map((cue, index) => {
      const startMs = Math.max(0, Math.round(cue.startMs));
      const endMs = Math.max(startMs, Math.round(cue.endMs));
      return `${index + 1}\n${formatSrtTime(startMs)} --> ${formatSrtTime(endMs)}\n${cue.text.trim()}\n`;
    })
    .join("\n");
}

export function buildFallbackCue(rawText: string): SubtitleCue[] {
  const text = rawText.trim();
  if (!text) return [];
  return [
    {
      id: cueId(1, 0),
      startMs: 0,
      endMs: 2_000,
      text,
      translatedText: "",
    },
  ];
}

export function createCueAfter(current?: SubtitleCue): SubtitleCue {
  const start = current ? current.endMs + 100 : 0;
  return {
    id: cueId(Date.now(), start),
    startMs: start,
    endMs: start + 2_000,
    text: "",
    translatedText: "",
  };
}

function cueId(seed: number, startMs: number): string {
  return `cue-${seed}-${startMs}-${Math.random().toString(36).slice(2, 7)}`;
}
