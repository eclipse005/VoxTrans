import type { SubtitleCue } from "./types";

const TIME_RE = /^(\d{2}):(\d{2}):(\d{2}),(\d{3})$/;

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

export function cuesToSrt(cues: SubtitleCue[]): string {
  return cues
    .map((cue, index) => {
      const startMs = Math.max(0, Math.round(cue.startMs));
      const endMs = Math.max(startMs, Math.round(cue.endMs));
      return `${index + 1}\n${formatSrtTime(startMs)} --> ${formatSrtTime(endMs)}\n${cue.text.trim()}\n`;
    })
    .join("\n");
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
