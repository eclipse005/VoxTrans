import type { SubtitleCue } from "../../features/media/types";

const CUE_NUMBER_RE = /\bcue\s+(\d+)\b/i;

function warningTextToZh(raw: string): string {
  const text = raw.trim();
  if (/has empty text/i.test(text)) return "文本为空";
  if (/has end before start/i.test(text)) return "结束时间早于开始时间";
  if (/is longer than 60 seconds/i.test(text)) return "时长超过 60 秒";
  const overlap = /overlaps with cue\s+(\d+)/i.exec(text);
  if (overlap) return `与第 ${overlap[1]} 条时间重叠`;
  return text;
}

export function buildCueWarningsById(cues: SubtitleCue[], warnings: string[]): Record<string, string[]> {
  const warningsById: Record<string, string[]> = {};
  const pushWarning = (cueId: string, warning: string) => {
    if (!warningsById[cueId]) warningsById[cueId] = [];
    warningsById[cueId].push(warning);
  };

  const ordered = [...cues].sort((a, b) => (a.startMs - b.startMs) || (a.endMs - b.endMs));
  for (const rawWarning of warnings) {
    const matched = CUE_NUMBER_RE.exec(rawWarning);
    if (!matched) continue;
    const cueNo = Number.parseInt(matched[1], 10);
    if (!Number.isFinite(cueNo) || cueNo <= 0) continue;
    const cue = ordered[cueNo - 1];
    if (!cue) continue;
    pushWarning(cue.id, warningTextToZh(rawWarning));
  }
  return warningsById;
}
